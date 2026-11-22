//! HDR output on the GPU: which surface format and color space a window
//! takes for HDR ([`choose_output`]), and the pass that encodes the frame
//! for it ([`HdrPass`]).
//!
//! With HDR on, the renderer draws everything (the 3D frame, sprites, UI)
//! into one `Rgba16Float` image where 1.0 is SDR (paper) white, instead of
//! into the window. [`HdrPass`] then draws that image into the window in
//! the display's encoding: scRGB, extended sRGB or PQ, or SDR when the
//! player turned HDR off while the game runs. Without HDR none of this
//! exists and the window is drawn as it always was.
//!
//! wgpu 30 does the platform work: it sets Metal's extended dynamic range
//! and the layer's color space, DXGI's `SetColorSpace1`, Vulkan's
//! `VkColorSpaceKHR` and the canvas's `toneMapping` from
//! `SurfaceConfiguration::color_space`, and reports the display's peak or
//! headroom (`Surface::display_hdr_info`). It writes no HDR metadata; the
//! engine sets it itself where the platform API is in reach
//! (`hdr_metadata.rs`).
//!
//! The inspector (egui) is authored for an sRGB window: it blends in linear
//! light into an sRGB image. With HDR output it draws into an image of the
//! window's SDR format instead (the overlay), which the output pass lays
//! over the frame before encoding it, so it looks as it does in SDR, at
//! paper white.

use super::hdr::{DisplayReport, HdrEncoding, HdrMetadata, HdrOffReason, HdrOutput, HdrStatus, SCRGB_NITS, brightness};

/// The surface format and color space HDR output takes on a window, and
/// how the frame is encoded for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputChoice {
    pub format: wgpu::TextureFormat,
    pub color_space: wgpu::SurfaceColorSpace,
    pub encoding: HdrEncoding,
    /// 1.0 is the system's SDR white (Apple's extended dynamic range, a
    /// browser's HDR canvas) rather than 80 nits: the system sets paper
    /// white, and its headroom is over it.
    pub relative: bool,
}

/// The HDR outputs to try on `backend`, best first: linear scRGB where the
/// system composites in it (Windows, macOS), PQ in 10 bits next, extended
/// sRGB on the web (the only HDR canvas there is).
fn candidates(backend: wgpu::Backend) -> &'static [OutputChoice] {
    use HdrEncoding::{ExtendedSrgb, Pq, ScRgb};
    use wgpu::SurfaceColorSpace as Cs;
    use wgpu::TextureFormat::{Rgb10a2Unorm, Rgba16Float};
    const fn c(format: wgpu::TextureFormat, color_space: Cs, encoding: HdrEncoding, relative: bool) -> OutputChoice {
        OutputChoice {
            format,
            color_space,
            encoding,
            relative,
        }
    }
    match backend {
        wgpu::Backend::Metal => {
            &const {
                [
                    c(Rgba16Float, Cs::ExtendedSrgbLinear, ScRgb, true),
                    c(Rgba16Float, Cs::ExtendedSrgb, ExtendedSrgb, true),
                    c(Rgb10a2Unorm, Cs::Bt2100Pq, Pq, false),
                ]
            }
        }
        wgpu::Backend::Dx12 => {
            &const {
                [
                    c(Rgba16Float, Cs::ExtendedSrgbLinear, ScRgb, false),
                    c(Rgb10a2Unorm, Cs::Bt2100Pq, Pq, false),
                ]
            }
        }
        wgpu::Backend::Vulkan => {
            &const {
                [
                    c(Rgba16Float, Cs::ExtendedSrgbLinear, ScRgb, false),
                    c(Rgb10a2Unorm, Cs::Bt2100Pq, Pq, false),
                    c(Rgba16Float, Cs::Bt2100Pq, Pq, false),
                    c(Rgba16Float, Cs::ExtendedSrgb, ExtendedSrgb, false),
                ]
            }
        }
        wgpu::Backend::BrowserWebGpu => &const { [c(Rgba16Float, Cs::ExtendedSrgb, ExtendedSrgb, true)] },
        // OpenGL/WebGL2 offer sRGB only; the no-op backend draws nothing.
        _ => &[],
    }
}

/// The HDR output a surface offering `formats` takes on `backend`, or none.
pub(crate) fn choose_output(
    backend: wgpu::Backend,
    formats: &[wgpu::SurfaceFormatCapabilities],
) -> Option<OutputChoice> {
    candidates(backend).iter().copied().find(|choice| {
        let flag = choice.color_space.to_color_spaces();
        formats
            .iter()
            .any(|f| f.format == choice.format && flag.is_some_and(|flag| f.color_spaces.contains(flag)))
    })
}

/// What wgpu says of the display, as the brightness math takes it.
pub(crate) fn display_report(info: &wgpu::DisplayHdrInfo) -> DisplayReport {
    DisplayReport {
        peak_nits: info.luminance.and_then(|l| l.max_nits),
        headroom: info.tone_map_headroom(),
    }
}

/// The output pass's settings, as `hdr_output.wgsl` reads them.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct OutputUniform {
    /// x: the encoding ([`HdrEncoding`] as a number); y: 1 when the
    /// overlay (the inspector) is laid over the frame.
    pub mode: [u32; 4],
    /// x: what 1.0 becomes before scRGB or extended sRGB encoding; y: paper
    /// white in nits (PQ).
    pub params: [f32; 4],
}

impl OutputUniform {
    /// SDR output: clamped to white, into an sRGB view.
    pub const SDR: Self = Self {
        mode: [HdrEncoding::Sdr as u32, 0, 0, 0],
        params: [1.0, 0.0, 0.0, 0.0],
    };

    /// `choice`'s encoding with paper white at `paper` nits.
    pub fn new(choice: &OutputChoice, paper: f32) -> Self {
        let scale = if choice.relative { 1.0 } else { paper / SCRGB_NITS };
        Self {
            mode: [choice.encoding as u32, 0, 0, 0],
            params: [scale, paper, 0.0, 0.0],
        }
    }

    /// With the overlay laid over the frame, or not.
    #[must_use]
    pub fn with_overlay(mut self, on: bool) -> Self {
        self.mode[1] = u32::from(on);
        self
    }
}

/// The window's HDR state: the settings, what the surface offered, and
/// (when the window opened with HDR on) the frame image and its pass.
pub(crate) struct Hdr {
    pub settings: HdrOutput,
    /// The HDR output the surface offered when the window opened.
    pub choice: Option<OutputChoice>,
    /// The surface's SDR format, for SDR output through the pass.
    pub sdr_format: wgpu::TextureFormat,
    /// Only when the window opened with HDR on and `choice` is some.
    pub pass: Option<HdrPass>,
    /// What the display reported last, and frames until it is asked again.
    pub display: DisplayReport,
    pub frames_to_query: u32,
    /// The HDR10 metadata the display was given last (`hdr_metadata.rs`),
    /// and whether the surface was configured since (wgpu may make a new
    /// swap chain then, which starts without).
    pub sent: Option<HdrMetadata>,
    pub resend: bool,
}

/// The display is asked again every this many frames (it moves between
/// screens, its brightness changes) and whenever the window is resized.
pub(crate) const QUERY_EVERY: u32 = 120;

impl Hdr {
    /// A window drawn the plain way: SDR, with what HDR would have taken.
    pub fn plain(settings: HdrOutput, choice: Option<OutputChoice>, sdr_format: wgpu::TextureFormat) -> Self {
        Self {
            settings,
            choice,
            sdr_format,
            pass: None,
            display: DisplayReport::default(),
            frames_to_query: 0,
            sent: None,
            resend: false,
        }
    }

    /// The overlay's format: the window's SDR format as an sRGB view
    /// draws it, the format the inspector draws in without HDR output.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn overlay_format(&self) -> wgpu::TextureFormat {
        self.sdr_format.add_srgb_suffix()
    }

    /// The HDR10 metadata the display should have now: while HDR is active
    /// in an encoding with absolute brightness (PQ, and scRGB where 1.0 is
    /// 80 nits); `None` otherwise.
    pub fn metadata(&self) -> Option<HdrMetadata> {
        let choice = self.choice.filter(|_| self.active())?;
        let absolute = matches!(choice.encoding, HdrEncoding::Pq) || !choice.relative;
        absolute.then(|| HdrMetadata::for_output(&self.settings, self.brightness().1))
    }

    /// Frames go out in HDR now.
    pub fn active(&self) -> bool {
        self.pass.is_some() && self.settings.enabled && self.choice.is_some()
    }

    /// The 3D frame's headroom over SDR white while HDR is active; `None`
    /// tone maps for SDR (exactly as without HDR).
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub fn tonemap_headroom(&self) -> Option<f32> {
        self.active().then(|| self.brightness().2)
    }

    /// Paper white, peak and headroom as used.
    pub fn brightness(&self) -> (f32, f32, f32) {
        brightness(&self.settings, self.choice.is_some_and(|c| c.relative), self.display)
    }

    /// The surface's format, color space and view format for the output
    /// as the settings are now (only while the pass exists).
    pub fn surface_target(&self) -> (wgpu::TextureFormat, wgpu::SurfaceColorSpace, wgpu::TextureFormat) {
        match self.choice.filter(|_| self.active()) {
            Some(c) => (c.format, c.color_space, c.format),
            None => (
                self.sdr_format,
                wgpu::SurfaceColorSpace::Auto,
                self.sdr_format.add_srgb_suffix(),
            ),
        }
    }

    /// The output pass's settings for the window.
    pub fn uniform(&self) -> OutputUniform {
        match self.choice.filter(|_| self.active()) {
            Some(c) => OutputUniform::new(&c, self.brightness().0),
            None => OutputUniform::SDR,
        }
    }

    /// What the game reads back (`Graphics::hdr_status`).
    pub fn status(&self) -> HdrStatus {
        let available = self.choice.is_some();
        let reason = if self.choice.is_none() {
            Some(HdrOffReason::Unsupported)
        } else if !self.settings.enabled {
            Some(HdrOffReason::Off)
        } else if self.pass.is_none() {
            // Only between the settings and the renderer taking them.
            Some(HdrOffReason::NotRequested)
        } else {
            None
        };
        let (paper, peak, headroom) = self.brightness();
        let mut status = match reason {
            Some(reason) => HdrStatus::sdr(available, reason),
            None => HdrStatus {
                available,
                active: true,
                encoding: self.choice.map_or(HdrEncoding::Sdr, |c| c.encoding),
                paper_white_nits: paper,
                peak_nits: peak,
                headroom,
                display_peak_nits: None,
                display_headroom: None,
                reason: None,
                metadata: self.sent,
            },
        };
        status.display_peak_nits = self.display.peak_nits;
        status.display_headroom = self.display.headroom;
        status
    }
}

/// The frame image HDR output draws into, and the pass that encodes it.
pub(crate) struct HdrPass {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    /// One pipeline per target format (the window's, a screenshot's).
    pipelines: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    /// The window's settings, and SDR for screenshots.
    uniform: wgpu::Buffer,
    sdr_uniform: wgpu::Buffer,
    /// What `uniform` holds.
    written: Option<OutputUniform>,
    /// Bound as the overlay where there is none: one transparent pixel.
    no_overlay: wgpu::TextureView,
    /// The frame image and the bind groups reading it (window, screenshot).
    frame: Option<FrameImage>,
}

struct FrameImage {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// The inspector's image, made the first time it draws.
    overlay: Option<(wgpu::Texture, wgpu::TextureView)>,
    window: wgpu::BindGroup,
    sdr: wgpu::BindGroup,
}

/// The frame image's format: linear, above 1 for highlights.
pub(crate) const FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Screenshots of an HDR frame are read back in SDR, in this format.
pub(crate) const SCREENSHOT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

impl HdrPass {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("hdr output layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hdr output pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("hdr_output.wgsl"));
        let buffer = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size_of::<OutputUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let uniform = buffer("hdr output settings");
        let sdr_uniform = buffer("hdr output settings (sdr)");
        queue.write_buffer(&sdr_uniform, 0, bytemuck::bytes_of(&OutputUniform::SDR));
        let no_overlay = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("hdr output: no overlay"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            layout,
            pipeline_layout,
            shader,
            pipelines: Vec::new(),
            uniform,
            sdr_uniform,
            written: None,
            no_overlay,
            frame: None,
        }
    }

    /// The pipeline drawing into `format`, made on first use.
    pub fn prepare_pipeline(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) {
        if self.pipelines.iter().any(|(f, _)| *f == format) {
            return;
        }
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hdr output"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs_full"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs_output"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.push((format, pipeline));
    }

    /// Writes the window's output settings when they changed.
    pub fn write(&mut self, queue: &wgpu::Queue, uniform: OutputUniform) {
        if self.written != Some(uniform) {
            queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
            self.written = Some(uniform);
        }
    }

    /// A view of the frame image at `size`, made again when the size changed.
    pub fn frame_view(&mut self, device: &wgpu::Device, (width, height): (u32, u32)) -> wgpu::TextureView {
        if let Some(f) = &self.frame
            && f.texture.width() == width
            && f.texture.height() == height
        {
            return f.view.clone();
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hdr frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FRAME_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let window = self.group(device, &view, &self.uniform, &self.no_overlay);
        let sdr = self.group(device, &view, &self.sdr_uniform, &self.no_overlay);
        self.frame = Some(FrameImage {
            texture,
            view: view.clone(),
            overlay: None,
            window,
            sdr,
        });
        view
    }

    /// A bind group reading `frame` with the settings in `buffer` and
    /// `overlay` laid over it.
    fn group(
        &self,
        device: &wgpu::Device,
        frame: &wgpu::TextureView,
        buffer: &wgpu::Buffer,
        overlay: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hdr output"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(frame),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(overlay),
                },
            ],
        })
    }

    /// The overlay image (in `format`, the frame image's size) the
    /// inspector draws into, made the first time; `None` before the frame
    /// image exists. The window's output pass lays it over the frame while
    /// its settings say so ([`OutputUniform::with_overlay`]); screenshots
    /// never show it.
    #[cfg_attr(not(feature = "inspector"), allow(dead_code))]
    pub fn overlay_view(&mut self, device: &wgpu::Device, format: wgpu::TextureFormat) -> Option<wgpu::TextureView> {
        let frame = self.frame.as_ref()?;
        if let Some((texture, view)) = &frame.overlay
            && texture.format() == format
        {
            return Some(view.clone());
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hdr overlay"),
            size: frame.texture.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let window = self.group(device, &frame.view, &self.uniform, &view);
        let frame = self.frame.as_mut()?;
        frame.window = window;
        frame.overlay = Some((texture, view.clone()));
        Some(view)
    }

    /// The overlay image, once made (the memory report).
    pub fn overlay_texture(&self) -> Option<&wgpu::Texture> {
        self.frame.as_ref()?.overlay.as_ref().map(|(t, _)| t)
    }

    /// Records the pass drawing the frame image into `target` (of `format`,
    /// whose pipeline was prepared): in the window's encoding, or in SDR
    /// (`sdr`, for screenshots).
    pub fn record(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        sdr: bool,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let (Some(frame), Some((_, pipeline))) = (&self.frame, self.pipelines.iter().find(|(f, _)| *f == format))
        else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("hdr output"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, if sdr { &frame.sdr } else { &frame.window }, &[]);
        pass.draw(0..3, 0..1);
    }

    /// The frame image, for the memory report and the GPU tests.
    pub fn frame_texture(&self) -> Option<&wgpu::Texture> {
        self.frame.as_ref().map(|f| &f.texture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::SurfaceColorSpaces as S;
    use wgpu::TextureFormat as F;

    fn caps(list: &[(F, S)]) -> Vec<wgpu::SurfaceFormatCapabilities> {
        list.iter()
            .map(|&(format, color_spaces)| wgpu::SurfaceFormatCapabilities { format, color_spaces })
            .collect()
    }

    /// What wgpu-hal 30's Metal backend reports (`metal/adapter.rs`).
    fn metal() -> Vec<wgpu::SurfaceFormatCapabilities> {
        let base = S::SRGB | S::DISPLAY_P3;
        let pq = S::BT2100_PQ | S::BT2100_HLG;
        caps(&[
            (F::Bgra8Unorm, base),
            (F::Bgra8UnormSrgb, base),
            (
                F::Rgba16Float,
                base | S::EXTENDED_SRGB_LINEAR | S::EXTENDED_SRGB | S::EXTENDED_DISPLAY_P3 | pq,
            ),
            (F::Rgb10a2Unorm, base | pq),
        ])
    }

    /// What wgpu-hal 30's DX12 backend reports (`dx12/adapter.rs`).
    fn dx12() -> Vec<wgpu::SurfaceFormatCapabilities> {
        caps(&[
            (F::Bgra8UnormSrgb, S::SRGB),
            (F::Bgra8Unorm, S::SRGB),
            (F::Rgba8UnormSrgb, S::SRGB),
            (F::Rgba8Unorm, S::SRGB),
            (F::Rgb10a2Unorm, S::SRGB | S::BT2100_PQ),
            (F::Rgba16Float, S::EXTENDED_SRGB_LINEAR),
        ])
    }

    #[test]
    fn macos_takes_extended_dynamic_range_in_scrgb() {
        let choice = choose_output(wgpu::Backend::Metal, &metal()).expect("Metal offers EDR");
        assert_eq!(
            (choice.format, choice.color_space, choice.encoding, choice.relative),
            (
                F::Rgba16Float,
                wgpu::SurfaceColorSpace::ExtendedSrgbLinear,
                HdrEncoding::ScRgb,
                true
            )
        );
    }

    #[test]
    fn windows_takes_scrgb_then_hdr10() {
        let choice = choose_output(wgpu::Backend::Dx12, &dx12()).expect("DXGI offers scRGB");
        assert_eq!(
            (choice.format, choice.encoding, choice.relative),
            (F::Rgba16Float, HdrEncoding::ScRgb, false)
        );
        let no_float = caps(&[(F::Bgra8UnormSrgb, S::SRGB), (F::Rgb10a2Unorm, S::SRGB | S::BT2100_PQ)]);
        let choice = choose_output(wgpu::Backend::Dx12, &no_float).expect("HDR10 next");
        assert_eq!(
            (choice.format, choice.color_space, choice.encoding),
            (F::Rgb10a2Unorm, wgpu::SurfaceColorSpace::Bt2100Pq, HdrEncoding::Pq)
        );
    }

    #[test]
    fn vulkan_takes_what_the_driver_exposes() {
        let sdr_only = caps(&[(F::Bgra8UnormSrgb, S::SRGB), (F::Bgra8Unorm, S::SRGB)]);
        assert_eq!(
            choose_output(wgpu::Backend::Vulkan, &sdr_only),
            None,
            "no HDR color space"
        );
        let hdr10 = caps(&[(F::Bgra8UnormSrgb, S::SRGB), (F::Rgb10a2Unorm, S::SRGB | S::BT2100_PQ)]);
        assert_eq!(
            choose_output(wgpu::Backend::Vulkan, &hdr10).map(|c| c.encoding),
            Some(HdrEncoding::Pq)
        );
        let both = caps(&[
            (F::Rgb10a2Unorm, S::BT2100_PQ),
            (F::Rgba16Float, S::EXTENDED_SRGB_LINEAR),
        ]);
        assert_eq!(
            choose_output(wgpu::Backend::Vulkan, &both).map(|c| c.encoding),
            Some(HdrEncoding::ScRgb),
            "scRGB before PQ"
        );
    }

    #[test]
    fn the_web_takes_the_extended_srgb_canvas_and_webgl2_nothing() {
        let webgpu = caps(&[
            (F::Bgra8Unorm, S::SRGB | S::DISPLAY_P3),
            (
                F::Rgba16Float,
                S::SRGB | S::DISPLAY_P3 | S::EXTENDED_SRGB | S::EXTENDED_DISPLAY_P3,
            ),
        ]);
        let choice = choose_output(wgpu::Backend::BrowserWebGpu, &webgpu).expect("an HDR canvas");
        assert_eq!(
            (choice.format, choice.color_space, choice.encoding, choice.relative),
            (
                F::Rgba16Float,
                wgpu::SurfaceColorSpace::ExtendedSrgb,
                HdrEncoding::ExtendedSrgb,
                true
            )
        );
        let canvas_without_hdr = caps(&[(F::Rgba16Float, S::SRGB | S::DISPLAY_P3)]);
        assert_eq!(choose_output(wgpu::Backend::BrowserWebGpu, &canvas_without_hdr), None);
        let gl = caps(&[(F::Rgba8UnormSrgb, S::SRGB), (F::Rgba16Float, S::SRGB)]);
        assert_eq!(choose_output(wgpu::Backend::Gl, &gl), None, "OpenGL reports sRGB only");
    }

    #[test]
    fn an_absolute_scrgb_output_scales_paper_white_from_80_nits() {
        let windows = choose_output(wgpu::Backend::Dx12, &dx12()).unwrap();
        assert_eq!(OutputUniform::new(&windows, 200.0).params[0], 2.5);
        let mac = choose_output(wgpu::Backend::Metal, &metal()).unwrap();
        assert_eq!(
            OutputUniform::new(&mac, 200.0).params[0],
            1.0,
            "macOS sets SDR white itself"
        );
    }

    #[test]
    fn hdr_status_says_why_it_is_off() {
        let windows = choose_output(wgpu::Backend::Dx12, &dx12());
        let plain = Hdr::plain(HdrOutput::on(), windows, F::Bgra8UnormSrgb);
        let status = plain.status();
        assert!(status.available && !status.active);
        assert_eq!(status.reason, Some(HdrOffReason::NotRequested));
        assert_eq!(plain.tonemap_headroom(), None, "SDR tone mapping");
        let mut off = HdrOutput::on();
        off.enabled = false;
        assert_eq!(
            Hdr::plain(off, windows, F::Bgra8UnormSrgb).status().reason,
            Some(HdrOffReason::Off),
            "off is off, whether or not the window ever was in HDR"
        );
        let none = Hdr::plain(HdrOutput::on(), None, F::Bgra8UnormSrgb);
        assert_eq!(none.status().reason, Some(HdrOffReason::Unsupported));
        assert_eq!(
            none.surface_target(),
            (F::Bgra8UnormSrgb, wgpu::SurfaceColorSpace::Auto, F::Bgra8UnormSrgb)
        );
    }
}
