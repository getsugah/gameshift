//! Minimal serial raster renderer. All handles are owned before fallible setup continues.
use crate::{
    protocol::RunConfig,
    vulkan::{self, Instance},
    FrameSample, ValidationMode,
};
use ash::vk;
use gpu_allocator::{
    vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator, AllocatorCreateDesc},
    MemoryLocation,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::{
    ffi::CString,
    io::Cursor,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};
use winit::window::Window;

type Result<T> = std::result::Result<T, String>;
const SHADOW_SIZE: u32 = 4096;
fn err(context: &str, e: impl std::fmt::Debug) -> String {
    format!("{context} : {e:?}")
}

struct Context {
    instance: Instance,
    surface_api: ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
    device: Option<ash::Device>,
}
impl Context {
    fn device(&self) -> &ash::Device {
        self.device
            .as_ref()
            .expect("logical device initialized before resources")
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: Renderer has destroyed all device children before Context is dropped.
        unsafe {
            if let Some(device) = self.device.take() {
                let _ = device.device_wait_idle();
                device.destroy_device(None);
            }
            if self.surface != vk::SurfaceKHR::null() {
                self.surface_api.destroy_surface(self.surface, None);
            }
        }
    }
}
struct Image {
    image: vk::Image,
    view: vk::ImageView,
    allocation: Option<Allocation>,
}
struct Buffer {
    buffer: vk::Buffer,
    allocation: Option<Allocation>,
}

pub(crate) struct Renderer {
    context: Context,
    allocator: Option<Allocator>,
    swap_api: Option<ash::khr::swapchain::Device>,
    swapchain: vk::SwapchainKHR,
    swap_images: Vec<vk::Image>,
    images: Vec<Image>,
    buffers: Vec<Buffer>,
    framebuffers: Vec<vk::Framebuffer>,
    render_pass: vk::RenderPass,
    pipeline: vk::Pipeline,
    sky_pipeline: vk::Pipeline,
    shadow_pipeline: vk::Pipeline,
    shadow_pass: vk::RenderPass,
    shadow_framebuffer: vk::Framebuffer,
    shadow_sampler: vk::Sampler,
    shadow_image: usize,
    pipeline_layout: vk::PipelineLayout,
    descriptor_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set: vk::DescriptorSet,
    sampler: vk::Sampler,
    shaders: Vec<vk::ShaderModule>,
    command_pool: vk::CommandPool,
    command: vk::CommandBuffer,
    fence: vk::Fence,
    acquired: vk::Semaphore,
    rendered: Vec<vk::Semaphore>,
    query_pool: vk::QueryPool,
    timestamp_bits: u32,
    timestamp_period: f32,
    queue: vk::Queue,
    extent: vk::Extent2D,
    presentation_extent: vk::Extent2D,
    pub sample_count: vk::SampleCountFlags,
    pub present_mode: String,
    pub validation_enabled: bool,
    pub validation_errors: Arc<AtomicUsize>,
    pub gpu_name: String,
    pub software: bool,
    pub driver_name: String,
    pub driver_info: String,
    pub pci_address: Option<String>,
}
impl Drop for Renderer {
    fn drop(&mut self) {
        let d = self.context.device();
        // SAFETY: No more commands are submitted; wait before destroying referenced resources.
        unsafe {
            let _ = d.device_wait_idle();
            for handle in self.framebuffers.drain(..) {
                d.destroy_framebuffer(handle, None);
            }
            d.destroy_framebuffer(self.shadow_framebuffer, None);
            d.destroy_pipeline(self.sky_pipeline, None);
            d.destroy_pipeline(self.shadow_pipeline, None);
            d.destroy_pipeline(self.pipeline, None);
            d.destroy_render_pass(self.shadow_pass, None);
            d.destroy_sampler(self.shadow_sampler, None);
            d.destroy_pipeline_layout(self.pipeline_layout, None);
            d.destroy_descriptor_pool(self.descriptor_pool, None);
            d.destroy_descriptor_set_layout(self.descriptor_layout, None);
            d.destroy_sampler(self.sampler, None);
            for shader in self.shaders.drain(..) {
                d.destroy_shader_module(shader, None);
            }
            d.destroy_render_pass(self.render_pass, None);
            d.destroy_query_pool(self.query_pool, None);
            d.destroy_fence(self.fence, None);
            d.destroy_semaphore(self.acquired, None);
            for s in self.rendered.drain(..) {
                d.destroy_semaphore(s, None);
            }
            d.destroy_command_pool(self.command_pool, None);
            if let Some(api) = &self.swap_api {
                api.destroy_swapchain(self.swapchain, None);
            }
            for image in self.images.drain(..) {
                d.destroy_image_view(image.view, None);
                d.destroy_image(image.image, None);
                if let (Some(allocator), Some(allocation)) = (&mut self.allocator, image.allocation)
                {
                    let _ = allocator.free(allocation);
                }
            }
            for buffer in self.buffers.drain(..) {
                d.destroy_buffer(buffer.buffer, None);
                if let (Some(allocator), Some(allocation)) =
                    (&mut self.allocator, buffer.allocation)
                {
                    let _ = allocator.free(allocation);
                }
            }
        }
        // Allocator must be destroyed before the logical device, even after setup errors.
        drop(self.allocator.take());
    }
}

impl Renderer {
    pub fn timestamp_metadata(&self) -> (Option<f32>, u32) {
        (
            (self.query_pool != vk::QueryPool::null()).then_some(self.timestamp_period),
            self.timestamp_bits,
        )
    }
    pub fn presentation_dimensions(&self) -> (u32, u32) {
        (
            self.presentation_extent.width,
            self.presentation_extent.height,
        )
    }
    pub fn dimensions(&self) -> (u32, u32) {
        (self.extent.width, self.extent.height)
    }
    pub fn new(config: &RunConfig, window: Option<&Window>) -> Result<Self> {
        Self::new_with_validation(config, window, None)
    }
    pub(crate) fn new_with_validation(
        config: &RunConfig,
        window: Option<&Window>,
        validation: Option<ValidationMode>,
    ) -> Result<Self> {
        config.validate()?;
        let extensions = if let Some(window) = window {
            ash_window::enumerate_required_extensions(
                window
                    .display_handle()
                    .map_err(|e| err("Affichage", e))?
                    .as_raw(),
            )
            .map_err(|e| err("Extensions de surface", e))?
            .to_vec()
        } else {
            vec![]
        };
        let mode = validation.unwrap_or(if config.require_validation {
            ValidationMode::Required
        } else {
            ValidationMode::IfAvailable
        });
        let (instance, _, version) =
            vulkan::create_instance(mode, &extensions).map_err(|e| e.to_string())?;
        if config.require_validation && !instance.monitored() {
            return Err("Messages de validation Vulkan indisponibles ; test refusé".into());
        }
        if version < vk::API_VERSION_1_1 {
            return Err("Vulkan 1.1 requis pour le rendu".into());
        }
        let surface_api = ash::khr::surface::Instance::new(&instance._entry, &instance.instance);
        let mut context = Context {
            instance,
            surface_api,
            surface: vk::SurfaceKHR::null(),
            device: None,
        };
        // SAFETY: Window/display handles remain alive until Renderer is destroyed by the runner.
        if let Some(window) = window {
            context.surface = unsafe {
                ash_window::create_surface(
                    &context.instance._entry,
                    &context.instance.instance,
                    window
                        .display_handle()
                        .map_err(|e| err("Affichage", e))?
                        .as_raw(),
                    window
                        .window_handle()
                        .map_err(|e| err("Fenêtre", e))?
                        .as_raw(),
                    None,
                )
            }
            .map_err(|e| err("Surface Vulkan", e))?;
        }
        let physicals = unsafe { context.instance.instance.enumerate_physical_devices() }
            .map_err(|e| err("GPU Vulkan", e))?;
        let mut selected = None;
        for p in physicals {
            let properties = unsafe { context.instance.instance.get_physical_device_properties(p) };
            if properties.api_version < vk::API_VERSION_1_1 {
                continue;
            }
            let mut ids = vk::PhysicalDeviceIDProperties::default();
            let mut props = vk::PhysicalDeviceProperties2::default().push_next(&mut ids);
            unsafe {
                context
                    .instance
                    .instance
                    .get_physical_device_properties2(p, &mut props);
            }
            let uuid: String = ids.device_uuid.iter().map(|b| format!("{b:02x}")).collect();
            if uuid.eq_ignore_ascii_case(&config.gpu_uuid) {
                selected = Some((p, properties));
                break;
            }
        }
        let (physical, properties) = selected
            .ok_or("Le GPU sélectionné n’est plus accessible, ou Vulkan 1.1 est indisponible")?;
        let (driver_name, driver_info) =
            if properties.api_version >= vk::API_VERSION_1_2 && version >= vk::API_VERSION_1_2 {
                let mut driver = vk::PhysicalDeviceDriverProperties::default();
                let mut extended = vk::PhysicalDeviceProperties2::default().push_next(&mut driver);
                unsafe {
                    context
                        .instance
                        .instance
                        .get_physical_device_properties2(physical, &mut extended);
                }
                (
                    vulkan::array_text(&driver.driver_name),
                    vulkan::array_text(&driver.driver_info),
                )
            } else {
                (
                    "Indisponible".into(),
                    format!("Version pilote brute : {}", properties.driver_version),
                )
            };
        let advertised = unsafe {
            context
                .instance
                .instance
                .enumerate_device_extension_properties(physical)
        }
        .map_err(|e| err("Extensions du GPU sélectionné", e))?;
        let pci_address = if advertised
            .iter()
            .any(|e| vulkan::array_text(&e.extension_name) == "VK_EXT_pci_bus_info")
        {
            let mut pci = vk::PhysicalDevicePCIBusInfoPropertiesEXT::default();
            let mut props = vk::PhysicalDeviceProperties2::default().push_next(&mut pci);
            unsafe {
                context
                    .instance
                    .instance
                    .get_physical_device_properties2(physical, &mut props);
            }
            Some(format!(
                "{:04x}:{:02x}:{:02x}.{:x}",
                pci.pci_domain, pci.pci_bus, pci.pci_device, pci.pci_function
            ))
        } else {
            None
        };
        let families = unsafe {
            context
                .instance
                .instance
                .get_physical_device_queue_family_properties(physical)
        };
        let mut family = None;
        for (index, q) in families.iter().enumerate() {
            if !q.queue_flags.contains(vk::QueueFlags::GRAPHICS) || q.queue_count == 0 {
                continue;
            }
            let supported = context.surface == vk::SurfaceKHR::null()
                || unsafe {
                    context.surface_api.get_physical_device_surface_support(
                        physical,
                        index as u32,
                        context.surface,
                    )
                }
                .map_err(|e| err("Support de présentation", e))?;
            if supported {
                family = Some(index as u32);
                break;
            }
        }
        let family = family
            .ok_or("Aucune file commune au rendu graphique et à la présentation sur ce GPU")?;
        let priorities = [1.0];
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(&priorities)];
        let device_extensions = if window.is_some() {
            vec![ash::khr::swapchain::NAME.as_ptr()]
        } else {
            vec![]
        };
        let info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queues)
            .enabled_extension_names(&device_extensions);
        context.device = Some(
            unsafe {
                context
                    .instance
                    .instance
                    .create_device(physical, &info, None)
            }
            .map_err(|e| err("Périphérique logique", e))?,
        );
        let allocator = Allocator::new(&AllocatorCreateDesc {
            instance: context.instance.instance.clone(),
            device: context.device().clone(),
            physical_device: physical,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        })
        .map_err(|e| err("Allocateur GPU", e))?;
        let queue = unsafe { context.device().get_device_queue(family, 0) };
        let validation_errors = context.instance.errors.clone();
        let validation_enabled = context.instance.validation_enabled;
        let mut r = Self {
            context,
            allocator: Some(allocator),
            swap_api: None,
            swapchain: vk::SwapchainKHR::null(),
            swap_images: vec![],
            images: vec![],
            buffers: vec![],
            framebuffers: vec![],
            render_pass: vk::RenderPass::null(),
            pipeline: vk::Pipeline::null(),
            sky_pipeline: vk::Pipeline::null(),
            shadow_pipeline: vk::Pipeline::null(),
            shadow_pass: vk::RenderPass::null(),
            shadow_framebuffer: vk::Framebuffer::null(),
            shadow_sampler: vk::Sampler::null(),
            shadow_image: 0,
            pipeline_layout: vk::PipelineLayout::null(),
            descriptor_layout: vk::DescriptorSetLayout::null(),
            descriptor_pool: vk::DescriptorPool::null(),
            descriptor_set: vk::DescriptorSet::null(),
            sampler: vk::Sampler::null(),
            shaders: vec![],
            command_pool: vk::CommandPool::null(),
            command: vk::CommandBuffer::null(),
            fence: vk::Fence::null(),
            acquired: vk::Semaphore::null(),
            rendered: vec![],
            query_pool: vk::QueryPool::null(),
            timestamp_bits: families[family as usize].timestamp_valid_bits,
            timestamp_period: properties.limits.timestamp_period,
            queue,
            extent: vk::Extent2D {
                width: config.width,
                height: config.height,
            },
            presentation_extent: window
                .map(|w| {
                    let size = w.inner_size();
                    vk::Extent2D {
                        width: size.width,
                        height: size.height,
                    }
                })
                .unwrap_or(vk::Extent2D {
                    width: config.width,
                    height: config.height,
                }),
            present_mode: "Hors écran".into(),
            sample_count: vk::SampleCountFlags::TYPE_1,
            validation_enabled,
            validation_errors,
            gpu_name: vulkan::array_text(&properties.device_name),
            driver_name,
            driver_info,
            pci_address,
            software: properties.device_type == vk::PhysicalDeviceType::CPU,
        };
        r.command_pool = unsafe {
            r.context.device().create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )
        }
        .map_err(|e| err("Command pool", e))?;
        r.command = unsafe {
            r.context.device().allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(r.command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
        }
        .map_err(|e| err("Command buffer", e))?[0];
        r.fence = unsafe {
            r.context.device().create_fence(
                &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                None,
            )
        }
        .map_err(|e| err("Fence", e))?;
        if r.timestamp_bits > 0 && r.timestamp_period.is_finite() && r.timestamp_period > 0.0 {
            r.query_pool = unsafe {
                r.context.device().create_query_pool(
                    &vk::QueryPoolCreateInfo::default()
                        .query_type(vk::QueryType::TIMESTAMP)
                        .query_count(2),
                    None,
                )
            }
            .map_err(|e| err("Timestamps", e))?;
        }
        let (format, target_views) = r.targets(physical)?;
        let depth_format = [vk::Format::D32_SFLOAT, vk::Format::D16_UNORM]
            .into_iter()
            .find(|format| {
                unsafe {
                    r.context
                        .instance
                        .instance
                        .get_physical_device_format_properties(physical, *format)
                }
                .optimal_tiling_features
                .contains(
                    vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT
                        | vk::FormatFeatureFlags::SAMPLED_IMAGE,
                )
            })
            .ok_or("Aucun format de profondeur compatible")?;
        let color_samples = unsafe {
            r.context
                .instance
                .instance
                .get_physical_device_image_format_properties(
                    physical,
                    format,
                    vk::ImageType::TYPE_2D,
                    vk::ImageTiling::OPTIMAL,
                    vk::ImageUsageFlags::COLOR_ATTACHMENT,
                    vk::ImageCreateFlags::empty(),
                )
        }
        .map_err(|e| err("Formats MSAA couleur", e))?
        .sample_counts;
        let depth_samples = unsafe {
            r.context
                .instance
                .instance
                .get_physical_device_image_format_properties(
                    physical,
                    depth_format,
                    vk::ImageType::TYPE_2D,
                    vk::ImageTiling::OPTIMAL,
                    vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                    vk::ImageCreateFlags::empty(),
                )
        }
        .map_err(|e| err("Formats MSAA profondeur", e))?
        .sample_counts;
        let supported = color_samples
            & depth_samples
            & properties.limits.framebuffer_color_sample_counts
            & properties.limits.framebuffer_depth_sample_counts;
        r.sample_count = [
            vk::SampleCountFlags::TYPE_16,
            vk::SampleCountFlags::TYPE_8,
            vk::SampleCountFlags::TYPE_4,
            vk::SampleCountFlags::TYPE_2,
            vk::SampleCountFlags::TYPE_1,
        ]
        .into_iter()
        .find(|count| count.as_raw() <= config.requested_msaa && supported.contains(*count))
        .ok_or("Aucun niveau MSAA commun couleur/profondeur")?;
        let depth = r.image_samples(
            depth_format,
            r.extent,
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            vk::ImageAspectFlags::DEPTH,
            r.sample_count,
        )?;
        let multisampled = if r.sample_count != vk::SampleCountFlags::TYPE_1 {
            Some(r.image_samples(
                format,
                r.extent,
                vk::ImageUsageFlags::COLOR_ATTACHMENT,
                vk::ImageAspectFlags::COLOR,
                r.sample_count,
            )?)
        } else {
            None
        };
        r.render_pass = r.make_render_pass(format, depth_format, false)?;
        for view in target_views {
            let attachments = if let Some(image) = multisampled {
                vec![r.images[image].view, r.images[depth].view, view]
            } else {
                vec![view, r.images[depth].view]
            };
            r.framebuffers.push(
                unsafe {
                    r.context.device().create_framebuffer(
                        &vk::FramebufferCreateInfo::default()
                            .render_pass(r.render_pass)
                            .attachments(&attachments)
                            .width(r.extent.width)
                            .height(r.extent.height)
                            .layers(1),
                        None,
                    )
                }
                .map_err(|e| err("Framebuffer MSAA", e))?,
            );
        }
        r.create_shadow(depth_format)?;
        r.texture()?;
        r.make_pipeline()?;
        Ok(r)
    }

    fn image(
        &mut self,
        format: vk::Format,
        extent: vk::Extent2D,
        usage: vk::ImageUsageFlags,
        aspect: vk::ImageAspectFlags,
    ) -> Result<usize> {
        self.image_samples(format, extent, usage, aspect, vk::SampleCountFlags::TYPE_1)
    }

    fn image_samples(
        &mut self,
        format: vk::Format,
        extent: vk::Extent2D,
        usage: vk::ImageUsageFlags,
        aspect: vk::ImageAspectFlags,
        samples: vk::SampleCountFlags,
    ) -> Result<usize> {
        let image = unsafe {
            self.context.device().create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D {
                        width: extent.width,
                        height: extent.height,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(samples)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )
        }
        .map_err(|e| err("Image GPU", e))?;
        let index = self.images.len();
        self.images.push(Image {
            image,
            view: vk::ImageView::null(),
            allocation: None,
        });
        let requirements = unsafe { self.context.device().get_image_memory_requirements(image) };
        let allocation = self
            .allocator
            .as_mut()
            .unwrap()
            .allocate(&AllocationCreateDesc {
                name: "gameshift-image",
                requirements,
                location: MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|e| err("Allocation image", e))?;
        self.images[index].allocation = Some(allocation);
        let allocation = self.images[index].allocation.as_ref().unwrap();
        unsafe {
            self.context
                .device()
                .bind_image_memory(image, allocation.memory(), allocation.offset())
        }
        .map_err(|e| err("Mémoire image", e))?;
        self.images[index].view = unsafe {
            self.context.device().create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(
                        vk::ImageSubresourceRange::default()
                            .aspect_mask(aspect)
                            .level_count(1)
                            .layer_count(1),
                    ),
                None,
            )
        }
        .map_err(|e| err("Vue image", e))?;
        Ok(index)
    }

    fn buffer(
        &mut self,
        size: u64,
        usage: vk::BufferUsageFlags,
        location: MemoryLocation,
    ) -> Result<usize> {
        let buffer = unsafe {
            self.context.device().create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )
        }
        .map_err(|e| err("Buffer", e))?;
        let index = self.buffers.len();
        self.buffers.push(Buffer {
            buffer,
            allocation: None,
        });
        let requirements = unsafe { self.context.device().get_buffer_memory_requirements(buffer) };
        let allocation = self
            .allocator
            .as_mut()
            .unwrap()
            .allocate(&AllocationCreateDesc {
                name: "gameshift-buffer",
                requirements,
                location,
                linear: true,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|e| err("Allocation buffer", e))?;
        self.buffers[index].allocation = Some(allocation);
        let a = self.buffers[index].allocation.as_ref().unwrap();
        unsafe {
            self.context
                .device()
                .bind_buffer_memory(buffer, a.memory(), a.offset())
        }
        .map_err(|e| err("Mémoire buffer", e))?;
        Ok(index)
    }

    fn targets(
        &mut self,
        physical: vk::PhysicalDevice,
    ) -> Result<(vk::Format, Vec<vk::ImageView>)> {
        if self.context.surface == vk::SurfaceKHR::null() {
            let image = self.image(
                vk::Format::R8G8B8A8_SRGB,
                self.extent,
                vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
                vk::ImageAspectFlags::COLOR,
            )?;
            return Ok((vk::Format::R8G8B8A8_SRGB, vec![self.images[image].view]));
        }
        let caps = unsafe {
            self.context
                .surface_api
                .get_physical_device_surface_capabilities(physical, self.context.surface)
        }
        .map_err(|e| err("Capacités surface", e))?;
        if !caps
            .supported_usage_flags
            .contains(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
        {
            return Err("Surface sans transfert de présentation compatible".into());
        }
        let formats = unsafe {
            self.context
                .surface_api
                .get_physical_device_surface_formats(physical, self.context.surface)
        }
        .map_err(|e| err("Formats surface", e))?;
        let mut format = formats
            .iter()
            .find(|f| {
                f.format == vk::Format::B8G8R8A8_SRGB
                    && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
            })
            .copied()
            .or_else(|| formats.first().copied())
            .ok_or("Aucun format de surface")?;
        if format.format == vk::Format::UNDEFINED {
            format.format = vk::Format::B8G8R8A8_SRGB;
        }
        if caps.current_extent.width != u32::MAX {
            self.presentation_extent = caps.current_extent;
        } else {
            self.presentation_extent.width = self
                .presentation_extent
                .width
                .clamp(caps.min_image_extent.width, caps.max_image_extent.width);
            self.presentation_extent.height = self
                .presentation_extent
                .height
                .clamp(caps.min_image_extent.height, caps.max_image_extent.height);
        }
        if self.presentation_extent.width == 0 || self.presentation_extent.height == 0 {
            return Err("Fenêtre minimisée ou surface vide".into());
        }
        let modes = unsafe {
            self.context
                .surface_api
                .get_physical_device_surface_present_modes(physical, self.context.surface)
        }
        .map_err(|e| err("Modes de présentation", e))?;
        let mode = if modes.contains(&vk::PresentModeKHR::IMMEDIATE) {
            vk::PresentModeKHR::IMMEDIATE
        } else if modes.contains(&vk::PresentModeKHR::MAILBOX) {
            vk::PresentModeKHR::MAILBOX
        } else {
            vk::PresentModeKHR::FIFO
        };
        self.present_mode = format!("{mode:?}");
        let count = if caps.max_image_count == 0 {
            (caps.min_image_count + 1).max(2)
        } else {
            (caps.min_image_count + 1).max(2).min(caps.max_image_count)
        };
        let alpha = [
            vk::CompositeAlphaFlagsKHR::OPAQUE,
            vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
            vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
            vk::CompositeAlphaFlagsKHR::INHERIT,
        ]
        .into_iter()
        .find(|a| caps.supported_composite_alpha.contains(*a))
        .ok_or("Composition de surface incompatible")?;
        let api = ash::khr::swapchain::Device::new(
            &self.context.instance.instance,
            self.context.device(),
        );
        self.swap_api = Some(api);
        let info = vk::SwapchainCreateInfoKHR::default()
            .surface(self.context.surface)
            .min_image_count(count)
            .image_format(format.format)
            .image_color_space(format.color_space)
            .image_extent(self.presentation_extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(caps.current_transform)
            .composite_alpha(alpha)
            .present_mode(mode)
            .clipped(true);
        self.swapchain = unsafe {
            self.swap_api
                .as_ref()
                .unwrap()
                .create_swapchain(&info, None)
        }
        .map_err(|e| err("Swapchain", e))?;
        self.swap_images = unsafe {
            self.swap_api
                .as_ref()
                .unwrap()
                .get_swapchain_images(self.swapchain)
        }
        .map_err(|e| err("Images de présentation", e))?;
        self.acquired = unsafe {
            self.context
                .device()
                .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
        }
        .map_err(|e| err("Sémaphore acquisition", e))?;
        for _ in &self.swap_images {
            self.rendered.push(
                unsafe {
                    self.context
                        .device()
                        .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                }
                .map_err(|e| err("Sémaphore présentation", e))?,
            );
        }
        let features = unsafe {
            self.context
                .instance
                .instance
                .get_physical_device_format_properties(physical, format.format)
        }
        .optimal_tiling_features;
        if !features.contains(
            vk::FormatFeatureFlags::BLIT_SRC
                | vk::FormatFeatureFlags::BLIT_DST
                | vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR,
        ) {
            return Err(
                "Le format ne permet pas le redimensionnement linéaire de la résolution interne"
                    .into(),
            );
        }
        let image = self.image(
            format.format,
            self.extent,
            vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
            vk::ImageAspectFlags::COLOR,
        )?;
        Ok((format.format, vec![self.images[image].view]))
    }

    fn make_render_pass(
        &self,
        color: vk::Format,
        depth: vk::Format,
        present: bool,
    ) -> Result<vk::RenderPass> {
        let final_layout = if present {
            vk::ImageLayout::PRESENT_SRC_KHR
        } else {
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL
        };
        let enabled = self.sample_count != vk::SampleCountFlags::TYPE_1;
        let mut attachments = vec![
            vk::AttachmentDescription::default()
                .format(color)
                .samples(self.sample_count)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(if enabled {
                    vk::AttachmentStoreOp::DONT_CARE
                } else {
                    vk::AttachmentStoreOp::STORE
                })
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(if enabled {
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL
                } else {
                    final_layout
                }),
            vk::AttachmentDescription::default()
                .format(depth)
                .samples(self.sample_count)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::DONT_CARE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
        ];
        if enabled {
            attachments.push(
                vk::AttachmentDescription::default()
                    .format(color)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .store_op(vk::AttachmentStoreOp::STORE)
                    .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                    .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .final_layout(final_layout),
            );
        }
        let colors = [vk::AttachmentReference {
            attachment: 0,
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        }];
        let depth_ref = vk::AttachmentReference {
            attachment: 1,
            layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        };
        let resolves = [vk::AttachmentReference {
            attachment: 2,
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        }];
        let mut subpass = vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&colors)
            .depth_stencil_attachment(&depth_ref);
        if enabled {
            subpass = subpass.resolve_attachments(&resolves);
        }
        let subpasses = [subpass];
        let dependencies = [
            vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                        | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                )
                .dst_stage_mask(
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                        | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                )
                .dst_access_mask(
                    vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                ),
            vk::SubpassDependency::default()
                .src_subpass(0)
                .dst_subpass(vk::SUBPASS_EXTERNAL)
                .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .dst_stage_mask(vk::PipelineStageFlags::TRANSFER)
                .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ),
        ];
        unsafe {
            self.context.device().create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(&attachments)
                    .subpasses(&subpasses)
                    .dependencies(&dependencies),
                None,
            )
        }
        .map_err(|e| err("Render pass", e))
    }

    fn begin(&self) -> Result<()> {
        unsafe {
            self.context
                .device()
                .wait_for_fences(&[self.fence], true, 5_000_000_000)
                .map_err(|e| err("Attente GPU (5 s)", e))?;
            self.context
                .device()
                .reset_command_buffer(self.command, vk::CommandBufferResetFlags::empty())
                .map_err(|e| err("Reset commandes", e))?;
            self.context
                .device()
                .begin_command_buffer(
                    self.command,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .map_err(|e| err("Début commandes", e))?;
        }
        Ok(())
    }

    fn submit(&self, wait: &[vk::Semaphore], signal: &[vk::Semaphore]) -> Result<()> {
        // Keep acquisition waits outside the timestamp interval. With one frame
        // in flight there is no earlier work to overlap with this wait.
        let stages = [vk::PipelineStageFlags::TOP_OF_PIPE];
        let commands = [self.command];
        let info = vk::SubmitInfo::default()
            .command_buffers(&commands)
            .wait_semaphores(wait)
            .wait_dst_stage_mask(if wait.is_empty() { &[] } else { &stages })
            .signal_semaphores(signal);
        unsafe {
            self.context
                .device()
                .end_command_buffer(self.command)
                .map_err(|e| err("Fin commandes", e))?;
            self.context
                .device()
                .reset_fences(&[self.fence])
                .map_err(|e| err("Reset fence", e))?;
            self.context
                .device()
                .queue_submit(self.queue, &[info], self.fence)
                .map_err(|e| err("Soumission GPU", e))?;
        }
        Ok(())
    }

    fn texture(&mut self) -> Result<()> {
        let texture = self.image(
            vk::Format::R8G8B8A8_UNORM,
            vk::Extent2D {
                width: 64,
                height: 64,
            },
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            vk::ImageAspectFlags::COLOR,
        )?;
        let staging = self.buffer(
            64 * 64 * 4,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
        )?;
        let allocation = self.buffers[staging].allocation.as_mut().unwrap();
        let bytes = allocation
            .mapped_slice_mut()
            .ok_or("Staging non mappable")?;
        for y in 0..64usize {
            for x in 0..64usize {
                let i = (y * 64 + x) * 4;
                let hash = (x as u32)
                    .wrapping_mul(374761393)
                    .wrapping_add((y as u32).wrapping_mul(668265263));
                let grain = ((hash ^ (hash >> 13)).wrapping_mul(1274126177) >> 24) as u8;
                let c = 180 + grain / 4;
                bytes[i..i + 4].copy_from_slice(&[c, c, c, 255]);
            }
        }
        let allocation = self.buffers[staging].allocation.as_ref().unwrap();
        if !allocation
            .memory_properties()
            .contains(vk::MemoryPropertyFlags::HOST_COHERENT)
        {
            unsafe {
                self.context
                    .device()
                    .flush_mapped_memory_ranges(&[vk::MappedMemoryRange::default()
                        .memory(allocation.memory())
                        .offset(0)
                        .size(vk::WHOLE_SIZE)])
            }
            .map_err(|e| err("Flush texture", e))?;
        }
        self.begin()?;
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let barrier = vk::ImageMemoryBarrier::default()
            .image(self.images[texture].image)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .subresource_range(range);
        unsafe {
            self.context.device().cmd_pipeline_barrier(
                self.command,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
            self.context.device().cmd_copy_buffer_to_image(
                self.command,
                self.buffers[staging].buffer,
                self.images[texture].image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::BufferImageCopy::default()
                    .image_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .image_extent(vk::Extent3D {
                        width: 64,
                        height: 64,
                        depth: 1,
                    })],
            );
            let barrier = barrier
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ);
            self.context.device().cmd_pipeline_barrier(
                self.command,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
        }
        self.submit(&[], &[])?;
        unsafe {
            self.context
                .device()
                .wait_for_fences(&[self.fence], true, 5_000_000_000)
        }
        .map_err(|e| err("Upload texture", e))?;
        self.sampler = unsafe {
            self.context.device().create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::LINEAR)
                    .min_filter(vk::Filter::LINEAR)
                    .address_mode_u(vk::SamplerAddressMode::REPEAT)
                    .address_mode_v(vk::SamplerAddressMode::REPEAT)
                    .address_mode_w(vk::SamplerAddressMode::REPEAT),
                None,
            )
        }
        .map_err(|e| err("Sampler", e))?;
        let bindings = [0, 1].map(|binding| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(binding)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)
        });
        self.descriptor_layout = unsafe {
            self.context.device().create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )
        }
        .map_err(|e| err("Descriptor layout", e))?;
        let sizes = [vk::DescriptorPoolSize {
            ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            descriptor_count: 2,
        }];
        self.descriptor_pool = unsafe {
            self.context.device().create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(1)
                    .pool_sizes(&sizes),
                None,
            )
        }
        .map_err(|e| err("Descriptor pool", e))?;
        self.descriptor_set = unsafe {
            self.context.device().allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(self.descriptor_pool)
                    .set_layouts(&[self.descriptor_layout]),
            )
        }
        .map_err(|e| err("Descriptor set", e))?[0];
        self.shadow_sampler = unsafe {
            self.context.device().create_sampler(
                &vk::SamplerCreateInfo::default()
                    .min_filter(vk::Filter::NEAREST)
                    .mag_filter(vk::Filter::NEAREST)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                    .border_color(vk::BorderColor::FLOAT_OPAQUE_WHITE),
                None,
            )
        }
        .map_err(|e| err("Sampler d’ombres", e))?;
        let infos = [vk::DescriptorImageInfo::default()
            .sampler(self.sampler)
            .image_view(self.images[texture].view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let shadows = [vk::DescriptorImageInfo::default()
            .sampler(self.shadow_sampler)
            .image_view(self.images[self.shadow_image].view)
            .image_layout(vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL)];
        unsafe {
            self.context.device().update_descriptor_sets(
                &[
                    vk::WriteDescriptorSet::default()
                        .dst_set(self.descriptor_set)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&infos),
                    vk::WriteDescriptorSet::default()
                        .dst_set(self.descriptor_set)
                        .dst_binding(1)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&shadows),
                ],
                &[],
            );
        }
        Ok(())
    }

    fn make_pipeline(&mut self) -> Result<()> {
        for bytes in [
            include_bytes!(concat!(env!("OUT_DIR"), "/city.vert.spv")).as_slice(),
            include_bytes!(concat!(env!("OUT_DIR"), "/city.frag.spv")).as_slice(),
            include_bytes!(concat!(env!("OUT_DIR"), "/sky.vert.spv")).as_slice(),
            include_bytes!(concat!(env!("OUT_DIR"), "/sky.frag.spv")).as_slice(),
        ] {
            let code = ash::util::read_spv(&mut Cursor::new(bytes))
                .map_err(|e| err("Shader SPIR-V", e))?;
            self.shaders.push(
                unsafe {
                    self.context.device().create_shader_module(
                        &vk::ShaderModuleCreateInfo::default().code(&code),
                        None,
                    )
                }
                .map_err(|e| err("Shader module", e))?,
            );
        }
        let entry = CString::new("main").unwrap();
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(self.shaders[0])
                .name(&entry),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(self.shaders[1])
                .name(&entry),
        ];
        let vertex = vk::PipelineVertexInputStateCreateInfo::default();
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewports = [vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: self.extent.width as f32,
            height: self.extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }];
        let scissors = [vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: self.extent,
        }];
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewports(&viewports)
            .scissors(&scissors);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::BACK)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(self.sample_count);
        let depth = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::LESS);
        let attachments = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
        let ranges = [vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::VERTEX,
            offset: 0,
            size: 128,
        }];
        self.pipeline_layout = unsafe {
            self.context.device().create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&[self.descriptor_layout])
                    .push_constant_ranges(&ranges),
                None,
            )
        }
        .map_err(|e| err("Pipeline layout", e))?;
        let pipeline = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .depth_stencil_state(&depth)
            .color_blend_state(&blend)
            .layout(self.pipeline_layout)
            .render_pass(self.render_pass)
            .subpass(0);
        self.pipeline = self.pipeline_handle(pipeline)?;
        let sky_stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(self.shaders[2])
                .name(&entry),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(self.shaders[3])
                .name(&entry),
        ];
        let sky_depth = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(false)
            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);
        let sky_raster = raster.cull_mode(vk::CullModeFlags::NONE);
        self.sky_pipeline = self.pipeline_handle(
            pipeline
                .stages(&sky_stages)
                .depth_stencil_state(&sky_depth)
                .rasterization_state(&sky_raster),
        )?;
        let shadow_stages = [stages[0]];
        let shadow_viewports = [vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: SHADOW_SIZE as f32,
            height: SHADOW_SIZE as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }];
        let shadow_scissors = [vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: SHADOW_SIZE,
                height: SHADOW_SIZE,
            },
        }];
        let shadow_viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewports(&shadow_viewports)
            .scissors(&shadow_scissors);
        let no_color = vk::PipelineColorBlendStateCreateInfo::default();
        let shadow_raster = raster
            .depth_bias_enable(true)
            .depth_bias_constant_factor(1.25)
            .depth_bias_slope_factor(1.75);
        let shadow_samples = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        self.shadow_pipeline = self.pipeline_handle(
            pipeline
                .multisample_state(&shadow_samples)
                .stages(&shadow_stages)
                .viewport_state(&shadow_viewport)
                .color_blend_state(&no_color)
                .rasterization_state(&shadow_raster)
                .render_pass(self.shadow_pass),
        )?;
        Ok(())
    }

    fn pipeline_handle(&self, info: vk::GraphicsPipelineCreateInfo<'_>) -> Result<vk::Pipeline> {
        match unsafe {
            self.context.device().create_graphics_pipelines(
                vk::PipelineCache::null(),
                &[info],
                None,
            )
        } {
            Ok(pipelines) => Ok(pipelines[0]),
            Err((pipelines, error)) => {
                for pipeline in pipelines {
                    unsafe {
                        self.context.device().destroy_pipeline(pipeline, None);
                    }
                }
                Err(err("Pipeline graphique", error))
            }
        }
    }

    fn create_shadow(&mut self, format: vk::Format) -> Result<()> {
        self.shadow_image = self.image(
            format,
            vk::Extent2D {
                width: SHADOW_SIZE,
                height: SHADOW_SIZE,
            },
            vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED,
            vk::ImageAspectFlags::DEPTH,
        )?;
        let attachments = [vk::AttachmentDescription::default()
            .format(format)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL)];
        let depth = vk::AttachmentReference {
            attachment: 0,
            layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        };
        let subpasses = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .depth_stencil_attachment(&depth)];
        let dependencies = [
            vk::SubpassDependency::default()
                .src_subpass(vk::SUBPASS_EXTERNAL)
                .dst_subpass(0)
                .src_stage_mask(vk::PipelineStageFlags::FRAGMENT_SHADER)
                .dst_stage_mask(
                    vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                        | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                )
                .src_access_mask(vk::AccessFlags::SHADER_READ)
                .dst_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE),
            vk::SubpassDependency::default()
                .src_subpass(0)
                .dst_subpass(vk::SUBPASS_EXTERNAL)
                .src_stage_mask(vk::PipelineStageFlags::LATE_FRAGMENT_TESTS)
                .dst_stage_mask(vk::PipelineStageFlags::FRAGMENT_SHADER)
                .src_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ),
        ];
        self.shadow_pass = unsafe {
            self.context.device().create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(&attachments)
                    .subpasses(&subpasses)
                    .dependencies(&dependencies),
                None,
            )
        }
        .map_err(|e| err("Pass d’ombres", e))?;
        let views = [self.images[self.shadow_image].view];
        self.shadow_framebuffer = unsafe {
            self.context.device().create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(self.shadow_pass)
                    .attachments(&views)
                    .width(SHADOW_SIZE)
                    .height(SHADOW_SIZE)
                    .layers(1),
                None,
            )
        }
        .map_err(|e| err("Framebuffer d’ombres", e))?;
        Ok(())
    }

    unsafe fn draw_objects(&self, config: &RunConfig) {
        // SAFETY: Caller has bound a compatible graphics pipeline in an active render pass.
        unsafe {
            self.context.device().cmd_draw(
                self.command,
                324,
                config.grid_side * config.grid_side * 8 + 1,
                0,
                0,
            );
            self.context
                .device()
                .cmd_draw(self.command, 64 * 16 * 6, 3, 0, 1_000_000);
            self.context
                .device()
                .cmd_draw(self.command, 324, 1, 0, 1_000_010);
            self.context
                .device()
                .cmd_draw(self.command, 324, 32, 0, 1_000_100);
        }
    }

    pub fn draw(&mut self, config: &RunConfig, seconds: f32) -> Result<FrameSample> {
        let frame_start = Instant::now();
        let index = if let Some(api) = &self.swap_api {
            let (index, suboptimal) = unsafe {
                api.acquire_next_image(
                    self.swapchain,
                    5_000_000_000,
                    self.acquired,
                    vk::Fence::null(),
                )
            }
            .map_err(|e| err("Acquisition image (test interrompu)", e))?;
            if suboptimal {
                return Err(
                    "La surface de rendu a changé ; relancer le test dans des conditions stables"
                        .into(),
                );
            }
            index as usize
        } else {
            0
        };
        self.begin()?;
        let cpu_start = Instant::now();
        let d = self.context.device();
        if self.query_pool != vk::QueryPool::null() {
            unsafe {
                d.cmd_reset_query_pool(self.command, self.query_pool, 0, 2);
                d.cmd_write_timestamp(
                    self.command,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    self.query_pool,
                    0,
                );
            }
        }
        let clear = [
            vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.035, 0.055, 0.09, 1.0],
                },
            },
            vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: 1.0,
                    stencil: 0,
                },
            },
        ];
        let angle = seconds * 0.075 + 0.65;
        let eye = glam::Vec3::new(
            angle.cos() * 9.5,
            6.3 + (seconds * 0.11).sin() * 0.35,
            angle.sin() * 9.5,
        );
        let view = glam::Mat4::look_at_rh(eye, glam::Vec3::new(0.0, 3.8, 0.0), glam::Vec3::Y);
        let mut projection = glam::Mat4::perspective_rh(
            63.0_f32.to_radians(),
            self.extent.width as f32 / self.extent.height as f32,
            0.1,
            350.0,
        );
        projection.y_axis.y *= -1.0;
        let matrix = (projection * view).to_cols_array();
        let span = config.grid_side as f32 * 2.3;
        let light_view = glam::Mat4::look_at_rh(
            glam::Vec3::new(-49.0, 80.0, 35.0),
            glam::Vec3::ZERO,
            glam::Vec3::Y,
        );
        let mut light_projection =
            glam::Mat4::orthographic_rh(-span, span, -span, span, 0.1, 220.0);
        light_projection.y_axis.y *= -1.0;
        let light = light_projection * light_view;
        let light_rows = [
            light.row(0).to_array(),
            light.row(1).to_array(),
            light.row(2).to_array(),
        ];
        let mut push = [0u8; 128];
        for (index, f) in matrix
            .into_iter()
            .chain([config.grid_side as f32, seconds, 0.0, 1.0])
            .chain(light_rows.into_iter().flatten())
            .enumerate()
        {
            push[index * 4..index * 4 + 4].copy_from_slice(&f.to_ne_bytes());
        }
        unsafe {
            d.cmd_begin_render_pass(
                self.command,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.shadow_pass)
                    .framebuffer(self.shadow_framebuffer)
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: vk::Extent2D {
                            width: SHADOW_SIZE,
                            height: SHADOW_SIZE,
                        },
                    })
                    .clear_values(&clear[1..]),
                vk::SubpassContents::INLINE,
            );
            d.cmd_bind_pipeline(
                self.command,
                vk::PipelineBindPoint::GRAPHICS,
                self.shadow_pipeline,
            );
            d.cmd_push_constants(
                self.command,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                &push,
            );
            self.draw_objects(config);
            d.cmd_end_render_pass(self.command);
        }
        push[76..80].copy_from_slice(&0f32.to_ne_bytes());
        unsafe {
            d.cmd_begin_render_pass(
                self.command,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.render_pass)
                    .framebuffer(self.framebuffers[0])
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: self.extent,
                    })
                    .clear_values(&clear),
                vk::SubpassContents::INLINE,
            );
            d.cmd_bind_pipeline(self.command, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            d.cmd_bind_descriptor_sets(
                self.command,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[self.descriptor_set],
                &[],
            );
            d.cmd_push_constants(
                self.command,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                &push,
            );
            self.draw_objects(config);
            d.cmd_bind_pipeline(
                self.command,
                vk::PipelineBindPoint::GRAPHICS,
                self.sky_pipeline,
            );
            d.cmd_draw(self.command, 3, 1, 0, 0);
            d.cmd_end_render_pass(self.command);
            if self.swap_api.is_some() {
                self.blit_for_presentation(index);
            }
            if self.query_pool != vk::QueryPool::null() {
                d.cmd_write_timestamp(
                    self.command,
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    self.query_pool,
                    1,
                );
            }
        }
        let wait_array = [self.acquired];
        let signal_array = [self
            .rendered
            .get(index)
            .copied()
            .unwrap_or(vk::Semaphore::null())];
        let (wait, signal): (&[vk::Semaphore], &[vk::Semaphore]) = if self.swap_api.is_some() {
            (&wait_array, &signal_array)
        } else {
            (&[], &[])
        };
        self.submit(wait, signal)?;
        let cpu_work_ms = cpu_start.elapsed().as_secs_f64() * 1000.0;
        unsafe { d.wait_for_fences(&[self.fence], true, 5_000_000_000) }
            .map_err(|e| err("Fin de frame GPU", e))?;
        let gpu_execution_ms = if self.query_pool != vk::QueryPool::null() {
            let mut times = [0u64; 2];
            unsafe {
                d.get_query_pool_results(
                    self.query_pool,
                    0,
                    &mut times,
                    vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
                )
            }
            .map_err(|e| err("Lecture timestamps GPU", e))?;
            let mask = if self.timestamp_bits >= 64 {
                u64::MAX
            } else {
                (1u64 << self.timestamp_bits) - 1
            };
            let wrap_ns =
                2.0_f64.powi(self.timestamp_bits as i32) * f64::from(self.timestamp_period);
            if self.timestamp_bits < 64 && frame_start.elapsed().as_secs_f64() * 1e9 >= wrap_ns {
                None
            } else {
                Some(
                    (times[1].wrapping_sub(times[0]) & mask) as f64
                        * f64::from(self.timestamp_period)
                        / 1_000_000.0,
                )
            }
        } else {
            None
        };
        if let Some(api) = &self.swap_api {
            let chains = [self.swapchain];
            let indices = [index as u32];
            let suboptimal = unsafe {
                api.queue_present(
                    self.queue,
                    &vk::PresentInfoKHR::default()
                        .wait_semaphores(signal)
                        .swapchains(&chains)
                        .image_indices(&indices),
                )
            }
            .map_err(|e| err("Présentation (test interrompu)", e))?;
            if suboptimal {
                return Err("Présentation devenue sous-optimale ; test interrompu".into());
            }
        }
        if self.validation_errors.load(Ordering::Relaxed) > 0 {
            return Err(
                "Erreur de validation Vulkan : résultats refusés, consulter les journaux".into(),
            );
        }
        Ok(FrameSample {
            frame_interval_ms: frame_start.elapsed().as_secs_f64() * 1000.0,
            cpu_work_ms: Some(cpu_work_ms),
            gpu_execution_ms,
        })
    }

    unsafe fn blit_for_presentation(&self, index: usize) {
        // SAFETY: Image was acquired, render pass resolved to images[0], all resources remain alive.
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let destination = vk::ImageMemoryBarrier::default()
            .image(self.swap_images[index])
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .subresource_range(range);
        unsafe {
            self.context.device().cmd_pipeline_barrier(
                self.command,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[destination],
            );
            let subresource = vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .layer_count(1);
            let region = vk::ImageBlit::default()
                .src_subresource(subresource)
                .dst_subresource(subresource)
                .src_offsets([
                    vk::Offset3D { x: 0, y: 0, z: 0 },
                    vk::Offset3D {
                        x: self.extent.width as i32,
                        y: self.extent.height as i32,
                        z: 1,
                    },
                ])
                .dst_offsets([
                    vk::Offset3D { x: 0, y: 0, z: 0 },
                    vk::Offset3D {
                        x: self.presentation_extent.width as i32,
                        y: self.presentation_extent.height as i32,
                        z: 1,
                    },
                ]);
            self.context.device().cmd_blit_image(
                self.command,
                self.images[0].image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                self.swap_images[index],
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
                vk::Filter::LINEAR,
            );
            let present = destination
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::empty());
            self.context.device().cmd_pipeline_barrier(
                self.command,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[present],
            );
        }
    }

    /// Read back one genuine offscreen frame for validation; never part of a timed run.
    pub fn readback(&mut self) -> Result<Vec<u8>> {
        if self.swap_api.is_some() {
            return Err("Readback réservé au test hors écran".into());
        }
        let size = u64::from(self.extent.width) * u64::from(self.extent.height) * 4;
        let buffer = self.buffer(
            size,
            vk::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuToCpu,
        )?;
        self.begin()?;
        unsafe {
            self.context.device().cmd_copy_image_to_buffer(
                self.command,
                self.images[0].image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                self.buffers[buffer].buffer,
                &[vk::BufferImageCopy::default()
                    .image_subresource(
                        vk::ImageSubresourceLayers::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1),
                    )
                    .image_extent(vk::Extent3D {
                        width: self.extent.width,
                        height: self.extent.height,
                        depth: 1,
                    })],
            );
            let host_read = vk::BufferMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::HOST_READ)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .buffer(self.buffers[buffer].buffer)
                .offset(0)
                .size(vk::WHOLE_SIZE);
            self.context.device().cmd_pipeline_barrier(
                self.command,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[],
                &[host_read],
                &[],
            );
        }
        self.submit(&[], &[])?;
        unsafe {
            self.context
                .device()
                .wait_for_fences(&[self.fence], true, 5_000_000_000)
        }
        .map_err(|e| err("Readback GPU", e))?;
        let a = self.buffers[buffer].allocation.as_ref().unwrap();
        if !a
            .memory_properties()
            .contains(vk::MemoryPropertyFlags::HOST_COHERENT)
        {
            unsafe {
                self.context.device().invalidate_mapped_memory_ranges(&[
                    vk::MappedMemoryRange::default()
                        .memory(a.memory())
                        .offset(0)
                        .size(vk::WHOLE_SIZE),
                ])
            }
            .map_err(|e| err("Invalidate readback", e))?;
        }
        Ok(a.mapped_slice().ok_or("Readback non mappable")?[..size as usize].to_vec())
    }
}
