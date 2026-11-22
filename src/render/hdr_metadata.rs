//! HDR10 static metadata (SMPTE ST 2086 mastering display, CTA-861.3
//! MaxCLL and MaxFALL) for the window, where the platform takes it.
//!
//! wgpu 30 sets the window's HDR color space but no metadata, so the
//! engine reaches the platform object behind wgpu's surface itself:
//!
//! | Platform | API | Sent for |
//! |---|---|---|
//! | Windows (DX12) | `IDXGISwapChain4::SetHDRMetaData` (HDR10) | PQ and scRGB |
//! | macOS, iOS (Metal) | `CAMetalLayer.EDRMetadata` (`CAEDRMetadata` HDR10) | PQ only: Apple's scRGB is relative to the system's SDR white, which the system tone maps itself |
//! | Vulkan | `vkSetHdrMetadataEXT` needs `VK_EXT_hdr_metadata`, which wgpu 30 doesn't enable on its device | none |
//! | Browsers (WebGPU) | no API for canvas metadata | none |
//!
//! [`send`] says whether the metadata went out; the renderer reports it in
//! `HdrStatus::metadata`. It is sent again when it changes and after every
//! surface configuration (wgpu may make a new swap chain then).

use super::hdr::{HdrEncoding, HdrMetadata};

/// CIE 1931 xy chromaticities: red, green, blue, white.
#[cfg_attr(not(windows), allow(dead_code))]
type Primaries = [[f32; 2]; 4];
/// BT.2020's primaries with D65 white (PQ output).
#[cfg_attr(not(windows), allow(dead_code))]
const BT2020: Primaries = [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046], [0.3127, 0.3290]];
/// BT.709's primaries with D65 white (scRGB output).
#[cfg_attr(not(windows), allow(dead_code))]
const BT709: Primaries = [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06], [0.3127, 0.3290]];

/// The HDR10 metadata in the units DXGI and HDMI carry it in.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hdr10 {
    /// Red, green, blue and white xy, in 0.00002 steps.
    pub primaries: [[u16; 2]; 4],
    /// The mastering display's peak, in nits.
    pub max_mastering: u32,
    /// The mastering display's black, in 0.0001 nits.
    pub min_mastering: u32,
    /// MaxCLL and MaxFALL, in nits.
    pub max_content: u16,
    pub max_frame_average: u16,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Hdr10 {
    /// `metadata` for output in `encoding` (its primaries).
    pub fn new(encoding: HdrEncoding, metadata: &HdrMetadata) -> Self {
        let primaries = if encoding == HdrEncoding::Pq { BT2020 } else { BT709 };
        let nits16 = |n: f32| n.round().clamp(0.0, f32::from(u16::MAX)) as u16;
        Self {
            primaries: primaries.map(|xy| xy.map(|c| (c * 50_000.0).round() as u16)),
            max_mastering: metadata.max_mastering_nits.round().max(0.0) as u32,
            min_mastering: (metadata.min_mastering_nits * 10_000.0).round().max(0.0) as u32,
            max_content: nits16(metadata.max_content_light_nits),
            max_frame_average: nits16(metadata.max_frame_average_nits),
        }
    }

    /// The bytes of DXGI's `DXGI_HDR_METADATA_HDR10` (28 bytes, little
    /// endian, no padding): what `SetHDRMetaData` reads.
    pub fn dxgi_bytes(&self) -> [u8; 28] {
        let mut out = [0; 28];
        for (i, v) in self.primaries.iter().flatten().enumerate() {
            out[2 * i..2 * i + 2].copy_from_slice(&v.to_le_bytes());
        }
        out[16..20].copy_from_slice(&self.max_mastering.to_le_bytes());
        out[20..24].copy_from_slice(&self.min_mastering.to_le_bytes());
        out[24..26].copy_from_slice(&self.max_content.to_le_bytes());
        out[26..28].copy_from_slice(&self.max_frame_average.to_le_bytes());
        out
    }
}

/// Gives the display behind `surface` (on `backend`, output in `encoding`)
/// the HDR10 `metadata`. Whether it went out: false where the platform
/// takes none (see the module's table) or the call failed.
pub(crate) fn send(
    surface: &wgpu::Surface<'_>,
    backend: wgpu::Backend,
    encoding: HdrEncoding,
    metadata: &HdrMetadata,
) -> bool {
    match backend {
        #[cfg(windows)]
        wgpu::Backend::Dx12 => dxgi::set_hdr_metadata(surface, &Hdr10::new(encoding, metadata)),
        #[cfg(target_vendor = "apple")]
        wgpu::Backend::Metal => encoding == HdrEncoding::Pq && apple::send(surface, metadata),
        _ => {
            let _ = (surface, encoding, metadata);
            false
        }
    }
}

#[cfg(windows)]
mod dxgi {
    use windows::Win32::Graphics::Dxgi::{DXGI_HDR_METADATA_TYPE_HDR10, IDXGISwapChain4};
    use windows::core::Interface as _;

    /// `SetHDRMetaData` on the swap chain wgpu made for `surface`.
    pub(super) fn set_hdr_metadata(surface: &wgpu::Surface<'_>, metadata: &super::Hdr10) -> bool {
        let bytes = metadata.dxgi_bytes();
        // SAFETY: the swap chain is wgpu's DX12 surface's own, borrowed
        // only for this call (the guard is dropped before wgpu is used
        // again); SetHDRMetaData reads the 28 bytes of a
        // DXGI_HDR_METADATA_HDR10 during the call and changes nothing wgpu
        // keeps track of.
        unsafe {
            let Some(hal) = surface.as_hal::<wgpu::hal::api::Dx12>() else {
                return false;
            };
            let Some(chain) = hal.swap_chain().and_then(|c| c.cast::<IDXGISwapChain4>().ok()) else {
                return false;
            };
            chain.SetHDRMetaData(DXGI_HDR_METADATA_TYPE_HDR10, Some(&bytes)).is_ok()
        }
    }
}

#[cfg(target_vendor = "apple")]
mod apple {
    use objc2_quartz_core::CAEDRMetadata;

    /// What 1.0 of the layer's content is in nits, as Apple's HDR10
    /// metadata takes it (Apple's own examples use 100 for PQ content).
    const OPTICAL_OUTPUT_SCALE: f32 = 100.0;

    /// `EDRMetadata` on the layer wgpu draws `surface` into.
    pub(super) fn send(surface: &wgpu::Surface<'_>, metadata: &crate::render::HdrMetadata) -> bool {
        let edr = CAEDRMetadata::HDR10MetadataWithMinLuminance_maxLuminance_opticalOutputScale(
            metadata.min_mastering_nits,
            metadata.max_mastering_nits,
            OPTICAL_OUTPUT_SCALE,
        );
        // SAFETY: the layer is wgpu's Metal surface's own, locked by its
        // mutex for this call only; setting its EDR metadata changes
        // nothing wgpu keeps track of.
        unsafe {
            let Some(hal) = surface.as_hal::<wgpu::hal::api::Metal>() else {
                return false;
            };
            hal.render_layer().lock().setEDRMetadata(Some(&edr));
        }
        true
    }

    /// Tests: the layer's EDR metadata is set.
    #[cfg(test)]
    pub(super) fn layer_has_metadata(surface: &wgpu::Surface<'_>) -> bool {
        // SAFETY: as in `send`: wgpu's own layer, read under its lock.
        unsafe {
            surface
                .as_hal::<wgpu::hal::api::Metal>()
                .is_some_and(|hal| hal.render_layer().lock().EDRMetadata().is_some())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::HdrOutput;

    fn metadata() -> HdrMetadata {
        let mut hdr = HdrOutput::on();
        hdr.max_frame_average_nits = 400.0;
        HdrMetadata::for_output(&hdr, 1000.0)
    }

    #[test]
    fn hdr10_metadata_carries_the_peak_and_the_encodings_primaries() {
        let pq = Hdr10::new(HdrEncoding::Pq, &metadata());
        assert_eq!(
            pq.primaries,
            [[35400, 14600], [8500, 39850], [6550, 2300], [15635, 16450]],
            "BT.2020 and D65 in 0.00002 steps"
        );
        assert_eq!(
            (pq.max_mastering, pq.min_mastering, pq.max_content, pq.max_frame_average),
            (1000, 0, 1000, 400)
        );
        let scrgb = Hdr10::new(HdrEncoding::ScRgb, &metadata());
        assert_eq!(scrgb.primaries[0], [32000, 16500], "BT.709 red for scRGB");
    }

    #[test]
    fn hdr10_bytes_follow_dxgis_layout() {
        let m = Hdr10::new(HdrEncoding::Pq, &metadata());
        let bytes = m.dxgi_bytes();
        assert_eq!(u16::from_le_bytes([bytes[0], bytes[1]]), 35400, "red x first");
        assert_eq!(
            u16::from_le_bytes([bytes[14], bytes[15]]),
            16450,
            "white y last of the primaries"
        );
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 1000);
        assert_eq!(u16::from_le_bytes([bytes[24], bytes[25]]), 1000);
        assert_eq!(u16::from_le_bytes([bytes[26], bytes[27]]), 400);
    }

    #[test]
    fn metadata_follows_the_settings_and_stays_under_the_peak() {
        let mut hdr = HdrOutput::on();
        let m = HdrMetadata::for_output(&hdr, 600.0);
        assert_eq!(
            (m.max_mastering_nits, m.max_content_light_nits, m.max_frame_average_nits),
            (600.0, 600.0, 200.0),
            "defaults: the peak and paper white"
        );
        hdr.max_content_light_nits = 5000.0;
        hdr.max_frame_average_nits = f32::NAN;
        let m = HdrMetadata::for_output(&hdr, 600.0);
        assert_eq!((m.max_content_light_nits, m.max_frame_average_nits), (600.0, 200.0));
    }

    /// The Metal path on a real `CAMetalLayer` with no window: configured
    /// for PQ, the layer takes the metadata; for scRGB nothing is sent.
    #[cfg(all(target_os = "macos", feature = "3d"))]
    #[test]
    fn a_metal_layer_configured_for_pq_takes_hdr10_metadata() {
        use objc2_quartz_core::CAMetalLayer;
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let layer = CAMetalLayer::new();
        // SAFETY: the layer outlives the surface (dropped at the end).
        let surface = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(
                std::ptr::from_ref::<CAMetalLayer>(&layer).cast_mut().cast(),
            ))
        }
        .expect("a surface on a CAMetalLayer");
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        })) else {
            crate::render::render_test_support::no_gpu(format_args!("no Metal adapter for a CAMetalLayer"));
            return;
        };
        let (device, _queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("a device");
        let caps = surface.get_capabilities(&adapter);
        let pq = crate::render::hdr_output::choose_output(wgpu::Backend::Metal, &caps.format_capabilities)
            .expect("Metal offers HDR");
        let mut config = surface
            .get_default_config(&adapter, 16, 16)
            .expect("the layer takes a configuration");
        config.format = wgpu::TextureFormat::Rgb10a2Unorm;
        config.color_space = wgpu::SurfaceColorSpace::Bt2100Pq;
        surface.configure(&device, &config);
        assert_eq!(pq.encoding, HdrEncoding::ScRgb, "scRGB is Metal's first choice");
        assert!(
            !send(&surface, wgpu::Backend::Metal, HdrEncoding::ScRgb, &metadata()),
            "relative scRGB takes none"
        );
        assert!(!apple::layer_has_metadata(&surface), "nothing was set");
        assert!(send(&surface, wgpu::Backend::Metal, HdrEncoding::Pq, &metadata()));
        assert!(apple::layer_has_metadata(&surface), "the layer holds it");
        drop(surface);
        drop(layer);
    }

    /// The DXGI path on a real DX12 swap chain, for a hidden window (it is
    /// never shown): configured for scRGB, the swap chain takes the
    /// metadata. Runs on Windows only.
    #[cfg(all(windows, feature = "3d"))]
    #[test]
    fn a_dxgi_swap_chain_takes_hdr10_metadata() {
        use winit::platform::windows::EventLoopBuilderExtWindows as _;
        let event_loop = winit::event_loop::EventLoop::builder()
            .with_any_thread(true)
            .build()
            .expect("an event loop");
        #[allow(deprecated)]
        let window = event_loop
            .create_window(winit::window::Window::default_attributes().with_visible(false))
            .expect("a hidden window");
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::DX12;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance.create_surface(&window).expect("a DX12 surface");
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        })) else {
            crate::render::render_test_support::no_gpu(format_args!("no DX12 adapter"));
            return;
        };
        let (device, _queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("a device");
        let caps = surface.get_capabilities(&adapter);
        let choice = crate::render::hdr_output::choose_output(wgpu::Backend::Dx12, &caps.format_capabilities)
            .expect("DXGI offers scRGB");
        let mut config = surface
            .get_default_config(&adapter, 16, 16)
            .expect("the window takes a configuration");
        config.format = choice.format;
        config.color_space = choice.color_space;
        surface.configure(&device, &config);
        assert!(
            send(&surface, wgpu::Backend::Dx12, choice.encoding, &metadata()),
            "SetHDRMetaData took it"
        );
    }
}
