use std::env;
#[cfg(all(feature = "linux-capture", target_os = "linux"))]
use std::time::{Duration, Instant};

use super::{
    CapabilityFailureReason, CapabilityResult, CapabilitySnapshot, CaptureCapabilities,
    DesktopPlatformCapabilities, Rect, RegionCapture, capability_failure, capability_success,
    desktop_os, detect_linux_session, observed_display_variables,
    platform_capture_unavailable_message,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SelectionCapture {
    pub(crate) anchor: Option<Rect>,
    pub(crate) source_app: Option<String>,
    pub(crate) text: Option<String>,
}

pub(crate) trait DesktopCaptureBackend {
    fn begin_region_picker(&self) -> CapabilityResult<Option<Rect>>;
    fn capture_region(&self, bounds: Rect) -> CapabilityResult<RegionCapture>;
    fn capabilities(&self) -> CaptureCapabilities;
    fn selection(&self) -> CapabilityResult<SelectionCapture>;
}

pub(crate) struct DesktopCaptureFacade {
    backend: DesktopCaptureBackendKind,
}

impl DesktopCaptureFacade {
    pub(crate) fn detect() -> Self {
        Self::from_env(
            env::var("XDG_SESSION_TYPE").ok().as_deref(),
            env::var("WAYLAND_DISPLAY").ok().as_deref(),
            env::var("DISPLAY").ok().as_deref(),
        )
    }

    pub(crate) fn from_env(
        xdg_session_type: Option<&str>,
        wayland_display: Option<&str>,
        display: Option<&str>,
    ) -> Self {
        if desktop_os() != "linux" {
            return Self {
                backend: DesktopCaptureBackendKind::Unsupported(UnsupportedBackend),
            };
        }
        match detect_linux_session(xdg_session_type, wayland_display, display) {
            "x11" => Self {
                backend: DesktopCaptureBackendKind::X11(X11CaptureBackend::new(
                    display.map(str::to_string),
                )),
            },
            "wayland" => Self {
                backend: DesktopCaptureBackendKind::Wayland(WaylandCaptureBackend),
            },
            _ => Self {
                backend: DesktopCaptureBackendKind::Unsupported(UnsupportedBackend),
            },
        }
    }

    pub(crate) fn begin_region_picker(&self) -> CapabilityResult<Option<Rect>> {
        self.backend.begin_region_picker()
    }

    pub(crate) fn capture_region(&self, bounds: Rect) -> CapabilityResult<RegionCapture> {
        self.backend.capture_region(bounds)
    }

    pub(crate) fn platform_capabilities(&self) -> DesktopPlatformCapabilities {
        DesktopPlatformCapabilities {
            capture: self.backend.capabilities(),
            display_variables: observed_display_variables(),
            os: desktop_os(),
            session: linux_session_from_env(),
        }
    }

    pub(crate) fn selection(&self) -> CapabilityResult<SelectionCapture> {
        self.backend.selection()
    }
}

enum DesktopCaptureBackendKind {
    Unsupported(UnsupportedBackend),
    Wayland(WaylandCaptureBackend),
    X11(X11CaptureBackend),
}

impl DesktopCaptureBackend for DesktopCaptureBackendKind {
    fn begin_region_picker(&self) -> CapabilityResult<Option<Rect>> {
        match self {
            Self::Unsupported(backend) => backend.begin_region_picker(),
            Self::Wayland(backend) => backend.begin_region_picker(),
            Self::X11(backend) => backend.begin_region_picker(),
        }
    }

    fn capture_region(&self, bounds: Rect) -> CapabilityResult<RegionCapture> {
        match self {
            Self::Unsupported(backend) => backend.capture_region(bounds),
            Self::Wayland(backend) => backend.capture_region(bounds),
            Self::X11(backend) => backend.capture_region(bounds),
        }
    }

    fn capabilities(&self) -> CaptureCapabilities {
        match self {
            Self::Unsupported(backend) => backend.capabilities(),
            Self::Wayland(backend) => backend.capabilities(),
            Self::X11(backend) => backend.capabilities(),
        }
    }

    fn selection(&self) -> CapabilityResult<SelectionCapture> {
        match self {
            Self::Unsupported(backend) => backend.selection(),
            Self::Wayland(backend) => backend.selection(),
            Self::X11(backend) => backend.selection(),
        }
    }
}

struct UnsupportedBackend;

impl DesktopCaptureBackend for UnsupportedBackend {
    fn begin_region_picker(&self) -> CapabilityResult<Option<Rect>> {
        capability_failure(
            "floating.beginRegionPicker",
            CapabilityFailureReason::Unavailable,
            platform_capture_unavailable_message(),
        )
    }

    fn capture_region(&self, _bounds: Rect) -> CapabilityResult<RegionCapture> {
        capability_failure(
            "floating.captureRegion",
            CapabilityFailureReason::Unavailable,
            platform_capture_unavailable_message(),
        )
    }

    fn capabilities(&self) -> CaptureCapabilities {
        let unavailable = unavailable_snapshot(platform_capture_unavailable_message());
        CaptureCapabilities {
            pointer: unavailable.clone(),
            portal_screenshot: unavailable.clone(),
            region_screenshot: unavailable.clone(),
            selection: unavailable,
        }
    }

    fn selection(&self) -> CapabilityResult<SelectionCapture> {
        capability_failure(
            "floating.currentSelection",
            CapabilityFailureReason::Unavailable,
            platform_capture_unavailable_message(),
        )
    }
}

struct X11CaptureBackend {
    display: Option<String>,
}

impl X11CaptureBackend {
    fn new(display: Option<String>) -> Self {
        Self { display }
    }

    fn pointer_anchor(&self) -> Option<Rect> {
        #[cfg(all(feature = "linux-capture", target_os = "linux"))]
        {
            use x11rb::connection::Connection;
            use x11rb::protocol::xproto::ConnectionExt as _;

            let (connection, screen_num) = x11rb::connect(self.display.as_deref()).ok()?;
            let root = connection.setup().roots.get(screen_num)?.root;
            let pointer = connection.query_pointer(root).ok()?.reply().ok()?;
            return Some(Rect {
                x: f64::from(pointer.root_x),
                y: f64::from(pointer.root_y),
                width: 1.0,
                height: 1.0,
            });
        }
        #[cfg(not(all(feature = "linux-capture", target_os = "linux")))]
        {
            let _ = &self.display;
            None
        }
    }

    #[cfg(all(feature = "linux-capture", target_os = "linux"))]
    fn primary_selection_text(&self) -> Result<Option<String>, (CapabilityFailureReason, String)> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{
            Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, WindowClass,
        };

        let (connection, screen_num) = x11rb::connect(self.display.as_deref()).map_err(|err| {
            (
                CapabilityFailureReason::Unavailable,
                format!("Linux X11 selection capture could not open the display: {err}"),
            )
        })?;
        let screen = connection.setup().roots.get(screen_num).ok_or_else(|| {
            (
                CapabilityFailureReason::Unavailable,
                "Linux X11 selection capture could not resolve the active screen.".to_string(),
            )
        })?;

        let primary = Atom::from(AtomEnum::PRIMARY);
        let owner = connection
            .get_selection_owner(primary)
            .map_err(|err| {
                (
                    CapabilityFailureReason::Unavailable,
                    format!("Linux X11 selection owner query failed: {err}"),
                )
            })?
            .reply()
            .map_err(|err| {
                (
                    CapabilityFailureReason::Unavailable,
                    format!("Linux X11 selection owner reply failed: {err}"),
                )
            })?
            .owner;
        if owner == x11rb::NONE {
            return Ok(None);
        }

        let utf8 = intern_atom(&connection, b"UTF8_STRING")?;
        let property = intern_atom(&connection, b"PSYCHEVO_SELECTION")?;
        let targets = [utf8, Atom::from(AtomEnum::STRING)];
        for target in targets {
            let requestor = connection.generate_id().map_err(|err| {
                (
                    CapabilityFailureReason::Failed,
                    format!("Linux X11 selection request window allocation failed: {err}"),
                )
            })?;
            connection
                .create_window(
                    screen.root_depth,
                    requestor,
                    screen.root,
                    0,
                    0,
                    1,
                    1,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    0,
                    &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
                )
                .map_err(|err| {
                    (
                        CapabilityFailureReason::Failed,
                        format!("Linux X11 selection request window creation failed: {err}"),
                    )
                })?;

            let result =
                request_selection_target(&connection, requestor, primary, target, property);
            let _ = connection.destroy_window(requestor);
            let _ = connection.flush();

            match result? {
                Some(text) if !text.trim().is_empty() => return Ok(Some(text)),
                _ => continue,
            }
        }

        Ok(None)
    }

    #[cfg(not(all(feature = "linux-capture", target_os = "linux")))]
    fn primary_selection_text(&self) -> Result<Option<String>, (CapabilityFailureReason, String)> {
        let _ = &self.display;
        Err((
            CapabilityFailureReason::Unavailable,
            "Linux X11 selection conversion support is not compiled into this build.".to_string(),
        ))
    }

    #[cfg(all(feature = "linux-capture", target_os = "linux"))]
    fn region_screenshot_data_url(
        &self,
        bounds: Rect,
    ) -> Result<String, (CapabilityFailureReason, String)> {
        use x11rb::connection::Connection;
        use x11rb::image::{Image, PixelLayout};

        let (connection, screen_num) = x11rb::connect(self.display.as_deref()).map_err(|err| {
            (
                CapabilityFailureReason::Unavailable,
                format!("Linux X11 screenshot capture could not open the display: {err}"),
            )
        })?;
        let screen = connection.setup().roots.get(screen_num).ok_or_else(|| {
            (
                CapabilityFailureReason::Unavailable,
                "Linux X11 screenshot capture could not resolve the active screen.".to_string(),
            )
        })?;
        let region = bounded_x11_region(bounds, screen.width_in_pixels, screen.height_in_pixels)
            .map_err(|message| (CapabilityFailureReason::Failed, message))?;
        let (image, visual_id) = Image::get(
            &connection,
            screen.root,
            region.x,
            region.y,
            region.width,
            region.height,
        )
        .map_err(|err| {
            (
                CapabilityFailureReason::Failed,
                format!("Linux X11 XGetImage request failed: {err}"),
            )
        })?;
        let visual = find_visual_type(connection.setup(), visual_id).ok_or_else(|| {
            (
                CapabilityFailureReason::Failed,
                "Linux X11 screenshot visual metadata was not available.".to_string(),
            )
        })?;
        let layout = PixelLayout::from_visual_type(visual).map_err(|err| {
            (
                CapabilityFailureReason::Failed,
                format!("Linux X11 screenshot visual format is unsupported: {err}"),
            )
        })?;
        let rgba = x11_image_to_rgba(&image, layout);
        encode_png_data_url(image.width(), image.height(), &rgba)
            .map_err(|message| (CapabilityFailureReason::Failed, message))
    }

    #[cfg(not(all(feature = "linux-capture", target_os = "linux")))]
    fn region_screenshot_data_url(
        &self,
        _bounds: Rect,
    ) -> Result<String, (CapabilityFailureReason, String)> {
        let _ = &self.display;
        Err((
            CapabilityFailureReason::Unavailable,
            "Linux X11 region screenshot support is not compiled into this build.".to_string(),
        ))
    }
}

impl DesktopCaptureBackend for X11CaptureBackend {
    fn begin_region_picker(&self) -> CapabilityResult<Option<Rect>> {
        let anchor = self.pointer_anchor();
        capability_success(anchor)
    }

    fn capture_region(&self, bounds: Rect) -> CapabilityResult<RegionCapture> {
        if let Some(capture) = capture_region_from_data_url(bounds) {
            return capture;
        }
        match self.region_screenshot_data_url(bounds) {
            Ok(data_url) => capability_success(RegionCapture {
                data_url,
                name: format!(
                    "floating-region-{}x{}.png",
                    bounds.width.round(),
                    bounds.height.round()
                ),
            }),
            Err((reason, message)) => capability_failure("floating.captureRegion", reason, message),
        }
    }

    fn capabilities(&self) -> CaptureCapabilities {
        let display_available = self.pointer_anchor().is_some();
        let pointer = if display_available {
            available_snapshot()
        } else {
            unavailable_snapshot("Linux X11 display is unavailable or cannot be opened.")
        };
        CaptureCapabilities {
            pointer,
            portal_screenshot: unsupported_snapshot(
                "Portal screenshots apply only to Linux Wayland sessions.",
            ),
            region_screenshot: if display_available {
                available_snapshot()
            } else {
                unavailable_snapshot("Linux X11 display is unavailable or cannot be opened.")
            },
            selection: if display_available {
                available_snapshot()
            } else {
                unavailable_snapshot("Linux X11 display is unavailable or cannot be opened.")
            },
        }
    }

    fn selection(&self) -> CapabilityResult<SelectionCapture> {
        let text = match env::var("PSYCHEVO_FLOATING_TEXT")
            .ok()
            .filter(|text| !text.trim().is_empty())
        {
            Some(text) => Some(text),
            None => match self.primary_selection_text() {
                Ok(text) => text,
                Err((reason, message)) => {
                    return capability_failure("floating.currentSelection", reason, message);
                }
            },
        };
        capability_success(SelectionCapture {
            anchor: self.pointer_anchor(),
            source_app: Some("Linux X11".to_string()),
            text,
        })
    }
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct X11Region {
    x: i16,
    y: i16,
    width: u16,
    height: u16,
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn intern_atom<C: x11rb::connection::Connection>(
    connection: &C,
    name: &[u8],
) -> Result<x11rb::protocol::xproto::Atom, (CapabilityFailureReason, String)> {
    use x11rb::protocol::xproto::ConnectionExt as _;

    connection
        .intern_atom(false, name)
        .map_err(|err| {
            (
                CapabilityFailureReason::Unavailable,
                format!("Linux X11 atom lookup request failed: {err}"),
            )
        })?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|err| {
            (
                CapabilityFailureReason::Unavailable,
                format!("Linux X11 atom lookup reply failed: {err}"),
            )
        })
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn request_selection_target<C: x11rb::connection::Connection>(
    connection: &C,
    requestor: x11rb::protocol::xproto::Window,
    selection: x11rb::protocol::xproto::Atom,
    target: x11rb::protocol::xproto::Atom,
    property: x11rb::protocol::xproto::Atom,
) -> Result<Option<String>, (CapabilityFailureReason, String)> {
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    connection
        .convert_selection(requestor, selection, target, property, x11rb::CURRENT_TIME)
        .map_err(|err| {
            (
                CapabilityFailureReason::Unavailable,
                format!("Linux X11 selection conversion request failed: {err}"),
            )
        })?;
    connection.flush().map_err(|err| {
        (
            CapabilityFailureReason::Unavailable,
            format!("Linux X11 selection conversion flush failed: {err}"),
        )
    })?;

    let deadline = Instant::now() + Duration::from_millis(250);
    while Instant::now() < deadline {
        if let Some(Event::SelectionNotify(event)) = connection.poll_for_event().map_err(|err| {
            (
                CapabilityFailureReason::Unavailable,
                format!("Linux X11 selection event polling failed: {err}"),
            )
        })? {
            if event.requestor != requestor
                || event.selection != selection
                || event.target != target
            {
                continue;
            }
            if event.property == x11rb::NONE {
                return Ok(None);
            }
            let reply = connection
                .get_property(
                    false,
                    requestor,
                    event.property,
                    event.target,
                    0,
                    (MAX_SELECTION_TEXT_BYTES / 4) as u32,
                )
                .map_err(|err| {
                    (
                        CapabilityFailureReason::Unavailable,
                        format!("Linux X11 selection property request failed: {err}"),
                    )
                })?
                .reply()
                .map_err(|err| {
                    (
                        CapabilityFailureReason::Unavailable,
                        format!("Linux X11 selection property reply failed: {err}"),
                    )
                })?;
            let _ = connection.delete_property(requestor, event.property);
            if reply.type_ == u32::from(AtomEnum::NONE)
                || reply.format != 8
                || reply.value.is_empty()
            {
                return Ok(None);
            }
            return Ok(selection_property_text(&reply.value));
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    Err((
        CapabilityFailureReason::Unavailable,
        "Linux X11 selection owner did not respond before the capture timeout.".to_string(),
    ))
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
const MAX_SELECTION_TEXT_BYTES: usize = 64 * 1024;

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn selection_property_text(bytes: &[u8]) -> Option<String> {
    let bounded = &bytes[..bytes.len().min(MAX_SELECTION_TEXT_BYTES)];
    let text = String::from_utf8_lossy(bounded)
        .trim_matches('\0')
        .to_string();
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn bounded_x11_region(
    bounds: Rect,
    screen_width: u16,
    screen_height: u16,
) -> Result<X11Region, String> {
    if !bounds.x.is_finite()
        || !bounds.y.is_finite()
        || !bounds.width.is_finite()
        || !bounds.height.is_finite()
        || bounds.width <= 0.0
        || bounds.height <= 0.0
    {
        return Err("Linux X11 screenshot bounds must be finite and positive.".to_string());
    }

    let screen_width = f64::from(screen_width);
    let screen_height = f64::from(screen_height);
    let left = bounds.x.floor().max(0.0);
    let top = bounds.y.floor().max(0.0);
    if left >= screen_width || top >= screen_height {
        return Err("Linux X11 screenshot bounds are outside the active screen.".to_string());
    }

    let right = (bounds.x + bounds.width)
        .ceil()
        .clamp(left + 1.0, screen_width);
    let bottom = (bounds.y + bounds.height)
        .ceil()
        .clamp(top + 1.0, screen_height);
    if left > f64::from(i16::MAX) || top > f64::from(i16::MAX) {
        return Err("Linux X11 screenshot origin exceeds X11 coordinate bounds.".to_string());
    }

    Ok(X11Region {
        x: left as i16,
        y: top as i16,
        width: (right - left).round().max(1.0).min(f64::from(u16::MAX)) as u16,
        height: (bottom - top).round().max(1.0).min(f64::from(u16::MAX)) as u16,
    })
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn find_visual_type(
    setup: &x11rb::protocol::xproto::Setup,
    visual_id: x11rb::protocol::xproto::Visualid,
) -> Option<x11rb::protocol::xproto::Visualtype> {
    setup
        .roots
        .iter()
        .flat_map(|screen| &screen.allowed_depths)
        .flat_map(|depth| &depth.visuals)
        .find(|visual| visual.visual_id == visual_id)
        .cloned()
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn x11_image_to_rgba(
    image: &x11rb::image::Image<'_>,
    layout: x11rb::image::PixelLayout,
) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(usize::from(image.width()) * usize::from(image.height()) * 4);
    for y in 0..image.height() {
        for x in 0..image.width() {
            let (red, green, blue) = layout.decode(image.get_pixel(x, y));
            rgba.push((red >> 8) as u8);
            rgba.push((green >> 8) as u8);
            rgba.push((blue >> 8) as u8);
            rgba.push(255);
        }
    }
    rgba
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn encode_png_data_url(width: u16, height: u16, rgba: &[u8]) -> Result<String, String> {
    use base64::Engine as _;

    let expected_len = usize::from(width) * usize::from(height) * 4;
    if rgba.len() != expected_len {
        return Err("Linux X11 screenshot pixel buffer size was invalid.".to_string());
    }

    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, u32::from(width), u32::from(height));
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|err| format!("Linux X11 screenshot PNG header failed: {err}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|err| format!("Linux X11 screenshot PNG encoding failed: {err}"))?;
    }

    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

struct WaylandCaptureBackend;

impl DesktopCaptureBackend for WaylandCaptureBackend {
    fn begin_region_picker(&self) -> CapabilityResult<Option<Rect>> {
        match self.require_area_target() {
            Ok(()) => capability_success(Some(Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            })),
            Err((reason, message)) => {
                capability_failure("floating.beginRegionPicker", reason, message)
            }
        }
    }

    fn capture_region(&self, bounds: Rect) -> CapabilityResult<RegionCapture> {
        if let Some(capture) = capture_region_from_data_url(bounds) {
            return capture;
        }
        match self.portal_area_screenshot_data_url() {
            Ok(data_url) => capability_success(RegionCapture {
                data_url,
                name: format!(
                    "floating-wayland-region-{}x{}.png",
                    bounds.width.round(),
                    bounds.height.round()
                ),
            }),
            Err((reason, message)) => capability_failure("floating.captureRegion", reason, message),
        }
    }

    fn capabilities(&self) -> CaptureCapabilities {
        let portal_screenshot = wayland_portal_snapshot();
        let region_screenshot = if portal_screenshot.available {
            portal_screenshot.clone()
        } else {
            match self.require_area_target() {
                Ok(()) => available_snapshot(),
                Err((reason, message)) => failure_snapshot(reason, message),
            }
        };
        CaptureCapabilities {
            pointer: unavailable_snapshot(
                "Linux Wayland pointer anchors depend on compositor-supported portals.",
            ),
            portal_screenshot,
            region_screenshot,
            selection: unavailable_snapshot(
                "Linux Wayland selected text requires AT-SPI support from the focused app.",
            ),
        }
    }

    fn selection(&self) -> CapabilityResult<SelectionCapture> {
        capability_failure(
            "floating.currentSelection",
            CapabilityFailureReason::Unavailable,
            "Linux Wayland selected text requires AT-SPI support from the focused app.",
        )
    }
}

impl WaylandCaptureBackend {
    fn require_area_target(&self) -> Result<(), (CapabilityFailureReason, String)> {
        #[cfg(all(feature = "linux-capture", target_os = "linux"))]
        {
            require_wayland_area_target(query_wayland_available_targets()?)
        }
        #[cfg(not(all(feature = "linux-capture", target_os = "linux")))]
        {
            Err((
                CapabilityFailureReason::Unavailable,
                "Linux Wayland portal support is not compiled into this build.".to_string(),
            ))
        }
    }

    fn portal_area_screenshot_data_url(&self) -> Result<String, (CapabilityFailureReason, String)> {
        #[cfg(all(feature = "linux-capture", target_os = "linux"))]
        {
            self.require_area_target()?;
            let uri = run_wayland_portal(async {
                let request = ashpd::desktop::screenshot::Screenshot::request()
                    .interactive(true)
                    .target(ashpd::desktop::screenshot::AvailableTargets::Area)
                    .send()
                    .await?;
                let screenshot = request.response()?;
                Ok(screenshot.uri().as_str().to_string())
            })?;
            portal_screenshot_uri_to_data_url(&uri)
                .map_err(|message| (CapabilityFailureReason::Failed, message))
        }
        #[cfg(not(all(feature = "linux-capture", target_os = "linux")))]
        {
            Err((
                CapabilityFailureReason::Unavailable,
                "Linux Wayland portal support is not compiled into this build.".to_string(),
            ))
        }
    }
}

fn wayland_portal_snapshot() -> CapabilitySnapshot {
    #[cfg(all(feature = "linux-capture", target_os = "linux"))]
    {
        return match query_wayland_available_targets().and_then(require_wayland_area_target) {
            Ok(()) => available_snapshot(),
            Err((reason, message)) => failure_snapshot(reason, message),
        };
    }
    #[cfg(not(all(feature = "linux-capture", target_os = "linux")))]
    {
        unavailable_snapshot("Linux Wayland portal support is not compiled into this build.")
    }
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
type WaylandTargets = ashpd::enumflags2::BitFlags<ashpd::desktop::screenshot::AvailableTargets>;

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn query_wayland_available_targets() -> Result<WaylandTargets, (CapabilityFailureReason, String)> {
    run_wayland_portal(async {
        let proxy = ashpd::desktop::screenshot::ScreenshotProxy::new().await?;
        proxy.available_targets().await
    })
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn require_wayland_area_target(
    targets: WaylandTargets,
) -> Result<(), (CapabilityFailureReason, String)> {
    if targets.contains(ashpd::desktop::screenshot::AvailableTargets::Area) {
        return Ok(());
    }
    Err((
        CapabilityFailureReason::Unavailable,
        format!(
            "XDG desktop portal does not advertise the Area screenshot target; advertised targets: {targets:?}."
        ),
    ))
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn run_wayland_portal<T, F>(future: F) -> Result<T, (CapabilityFailureReason, String)>
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, ashpd::Error>> + Send + 'static,
{
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| {
                (
                    CapabilityFailureReason::Failed,
                    format!("Linux Wayland portal runtime initialization failed: {err}"),
                )
            })
            .and_then(|runtime| runtime.block_on(future).map_err(map_ashpd_error));
        let _ = sender.send(result);
    });

    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|err| match err {
            std::sync::mpsc::RecvTimeoutError::Timeout => (
                CapabilityFailureReason::Unavailable,
                "Linux Wayland portal did not respond before the capture timeout.".to_string(),
            ),
            std::sync::mpsc::RecvTimeoutError::Disconnected => (
                CapabilityFailureReason::Failed,
                "Linux Wayland portal worker exited before returning a result.".to_string(),
            ),
        })?
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn map_ashpd_error(error: ashpd::Error) -> (CapabilityFailureReason, String) {
    use ashpd::desktop::ResponseError;
    use ashpd::{Error, PortalError};

    let reason = match &error {
        Error::Response(ResponseError::Cancelled) => CapabilityFailureReason::Canceled,
        Error::Portal(PortalError::Cancelled(_)) => CapabilityFailureReason::Canceled,
        Error::Portal(PortalError::NotAllowed(_)) => CapabilityFailureReason::PermissionDenied,
        Error::PortalNotFound(_) | Error::RequiresVersion(_, _) => {
            CapabilityFailureReason::Unavailable
        }
        Error::Portal(PortalError::NotFound(_)) => CapabilityFailureReason::Unavailable,
        _ => CapabilityFailureReason::Failed,
    };
    (
        reason,
        format!("Linux Wayland portal screenshot failed: {error}"),
    )
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn portal_screenshot_uri_to_data_url(uri: &str) -> Result<String, String> {
    use base64::Engine as _;

    let path = file_uri_to_path(uri)?;
    let bytes = std::fs::read(&path)
        .map_err(|err| format!("Linux Wayland portal screenshot file could not be read: {err}"))?;
    let mime = match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") | None => "image/png",
        Some("webp") => "image/webp",
        Some(other) => {
            return Err(format!(
                "Linux Wayland portal screenshot file type is unsupported: {other}"
            ));
        }
    };

    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn file_uri_to_path(uri: &str) -> Result<std::path::PathBuf, String> {
    let path = uri
        .strip_prefix("file://")
        .ok_or_else(|| format!("Linux Wayland portal returned a non-file screenshot URI: {uri}"))?;
    let path = path
        .strip_prefix("localhost/")
        .map(|local| format!("/{local}"))
        .unwrap_or_else(|| path.to_string());
    if !path.starts_with('/') {
        return Err(format!(
            "Linux Wayland portal returned a file URI with an unsupported host: {uri}"
        ));
    }
    Ok(std::path::PathBuf::from(percent_decode_uri_path(&path)?))
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn percent_decode_uri_path(path: &str) -> Result<String, String> {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hi = bytes.get(index + 1).copied().ok_or_else(|| {
                "Linux Wayland portal screenshot URI has an incomplete percent escape.".to_string()
            })?;
            let lo = bytes.get(index + 2).copied().ok_or_else(|| {
                "Linux Wayland portal screenshot URI has an incomplete percent escape.".to_string()
            })?;
            decoded.push(hex_value(hi)? * 16 + hex_value(lo)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|err| {
        format!("Linux Wayland portal screenshot URI path is not valid UTF-8: {err}")
    })
}

#[cfg(all(feature = "linux-capture", target_os = "linux"))]
fn hex_value(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err("Linux Wayland portal screenshot URI has an invalid percent escape.".to_string()),
    }
}

fn available_snapshot() -> CapabilitySnapshot {
    CapabilitySnapshot {
        available: true,
        message: None,
        reason: None,
    }
}

fn failure_snapshot(
    reason: CapabilityFailureReason,
    message: impl Into<String>,
) -> CapabilitySnapshot {
    CapabilitySnapshot {
        available: false,
        message: Some(message.into()),
        reason: Some(reason),
    }
}

fn capture_region_from_data_url(bounds: Rect) -> Option<CapabilityResult<RegionCapture>> {
    let data_url = env::var("PSYCHEVO_FLOATING_REGION_DATA_URL").ok()?;
    data_url.starts_with("data:image/").then(|| {
        capability_success(RegionCapture {
            data_url,
            name: format!(
                "floating-region-{}x{}.png",
                bounds.width.round(),
                bounds.height.round()
            ),
        })
    })
}

fn unavailable_snapshot(message: impl Into<String>) -> CapabilitySnapshot {
    CapabilitySnapshot {
        available: false,
        message: Some(message.into()),
        reason: Some(CapabilityFailureReason::Unavailable),
    }
}

fn unsupported_snapshot(message: impl Into<String>) -> CapabilitySnapshot {
    CapabilitySnapshot {
        available: false,
        message: Some(message.into()),
        reason: Some(CapabilityFailureReason::Unsupported),
    }
}

fn linux_session_from_env() -> &'static str {
    if desktop_os() != "linux" {
        return "unknown";
    }
    detect_linux_session(
        env::var("XDG_SESSION_TYPE").ok().as_deref(),
        env::var("WAYLAND_DISPLAY").ok().as_deref(),
        env::var("DISPLAY").ok().as_deref(),
    )
}

#[cfg(test)]
pub(crate) struct FakeCaptureBackend {
    pub(crate) pointer: Option<Rect>,
    pub(crate) region_data_url: Option<String>,
    pub(crate) selection_text: Option<String>,
}

#[cfg(test)]
impl DesktopCaptureBackend for FakeCaptureBackend {
    fn begin_region_picker(&self) -> CapabilityResult<Option<Rect>> {
        capability_success(self.pointer)
    }

    fn capture_region(&self, bounds: Rect) -> CapabilityResult<RegionCapture> {
        match &self.region_data_url {
            Some(data_url) => capability_success(RegionCapture {
                data_url: data_url.clone(),
                name: format!(
                    "floating-region-{}x{}.png",
                    bounds.width.round(),
                    bounds.height.round()
                ),
            }),
            None => capability_failure(
                "floating.captureRegion",
                CapabilityFailureReason::Unavailable,
                "fake screenshot unavailable",
            ),
        }
    }

    fn capabilities(&self) -> CaptureCapabilities {
        CaptureCapabilities {
            pointer: self
                .pointer
                .map(|_| available_snapshot())
                .unwrap_or_else(|| unavailable_snapshot("fake pointer unavailable")),
            portal_screenshot: unsupported_snapshot("fake backend has no portal"),
            region_screenshot: self
                .region_data_url
                .as_ref()
                .map(|_| available_snapshot())
                .unwrap_or_else(|| unavailable_snapshot("fake screenshot unavailable")),
            selection: self
                .selection_text
                .as_ref()
                .map(|_| available_snapshot())
                .unwrap_or_else(|| unavailable_snapshot("fake selection unavailable")),
        }
    }

    fn selection(&self) -> CapabilityResult<SelectionCapture> {
        capability_success(SelectionCapture {
            anchor: self.pointer,
            source_app: Some("Fake app".to_string()),
            text: self.selection_text.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_selects_linux_backend_family_from_environment() {
        let x11 = DesktopCaptureFacade::from_env(Some("x11"), Some("wayland-0"), Some(":0"));
        assert!(matches!(x11.backend, DesktopCaptureBackendKind::X11(_)));

        let wayland = DesktopCaptureFacade::from_env(None, Some("wayland-0"), Some(":0"));
        assert!(matches!(
            wayland.backend,
            DesktopCaptureBackendKind::Wayland(_)
        ));

        let unknown = DesktopCaptureFacade::from_env(None, None, None);
        assert!(matches!(
            unknown.backend,
            DesktopCaptureBackendKind::Unsupported(_)
        ));
    }

    #[test]
    fn fake_backend_maps_selection_pointer_and_screenshot() {
        let backend = FakeCaptureBackend {
            pointer: Some(Rect {
                x: 11.0,
                y: 22.0,
                width: 1.0,
                height: 1.0,
            }),
            region_data_url: Some("data:image/png;base64,AA==".to_string()),
            selection_text: Some("selected text".to_string()),
        };

        let selection = match backend.selection() {
            CapabilityResult::Success(success) => success.value,
            CapabilityResult::Failure(failure) => panic!("{failure:?}"),
        };
        assert_eq!(selection.anchor.unwrap().x, 11.0);
        assert_eq!(selection.source_app.as_deref(), Some("Fake app"));
        assert_eq!(selection.text.as_deref(), Some("selected text"));

        let screenshot = match backend.capture_region(Rect {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 40.0,
        }) {
            CapabilityResult::Success(success) => success.value,
            CapabilityResult::Failure(failure) => panic!("{failure:?}"),
        };
        assert_eq!(screenshot.name, "floating-region-80x40.png");
        assert_eq!(screenshot.data_url, "data:image/png;base64,AA==");
    }

    #[test]
    fn wayland_selection_does_not_claim_x11_selection_parity() {
        let selection = WaylandCaptureBackend.selection();
        let CapabilityResult::Failure(failure) = selection else {
            panic!("Wayland selected text should require AT-SPI support")
        };

        assert_eq!(failure.reason, CapabilityFailureReason::Unavailable);
        assert!(failure.message.unwrap().contains("AT-SPI"));
    }

    #[cfg(all(feature = "linux-capture", target_os = "linux"))]
    #[test]
    fn x11_region_bounds_are_clamped_to_screen() {
        let region = bounded_x11_region(
            Rect {
                x: -10.4,
                y: 5.2,
                width: 40.7,
                height: 30.1,
            },
            100,
            50,
        )
        .expect("region is valid");

        assert_eq!(
            region,
            X11Region {
                x: 0,
                y: 5,
                width: 31,
                height: 31,
            }
        );

        let edge = bounded_x11_region(
            Rect {
                x: 95.0,
                y: 45.0,
                width: 50.0,
                height: 50.0,
            },
            100,
            50,
        )
        .expect("edge region is valid");
        assert_eq!(edge.width, 5);
        assert_eq!(edge.height, 5);
    }

    #[cfg(all(feature = "linux-capture", target_os = "linux"))]
    #[test]
    fn x11_selection_property_text_is_bounded_and_trimmed() {
        assert_eq!(
            selection_property_text(b"selected text\0\0").as_deref(),
            Some("selected text")
        );

        let large = vec![b'a'; MAX_SELECTION_TEXT_BYTES + 512];
        let text = selection_property_text(&large).expect("text is present");
        assert_eq!(text.len(), MAX_SELECTION_TEXT_BYTES);
    }

    #[cfg(all(feature = "linux-capture", target_os = "linux"))]
    #[test]
    fn x11_png_encoder_returns_data_url() {
        let rgba = [
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let data_url = encode_png_data_url(2, 2, &rgba).expect("png encodes");

        assert!(data_url.starts_with("data:image/png;base64,"));
        assert!(data_url.len() > "data:image/png;base64,".len());
    }

    #[cfg(all(feature = "linux-capture", target_os = "linux"))]
    #[test]
    fn wayland_area_target_mapping_distinguishes_success_and_missing_target() {
        use ashpd::desktop::screenshot::AvailableTargets;

        let area: WaylandTargets = AvailableTargets::Area.into();
        assert!(require_wayland_area_target(area).is_ok());

        let screen: WaylandTargets = AvailableTargets::Screen.into();
        let (reason, message) =
            require_wayland_area_target(screen).expect_err("area target is missing");
        assert_eq!(reason, CapabilityFailureReason::Unavailable);
        assert!(message.contains("does not advertise the Area screenshot target"));
    }

    #[cfg(all(feature = "linux-capture", target_os = "linux"))]
    #[test]
    fn wayland_portal_errors_map_to_capability_reasons() {
        use ashpd::desktop::ResponseError;
        use ashpd::zbus::names::OwnedInterfaceName;
        use ashpd::{Error, PortalError};

        let (reason, _) = map_ashpd_error(Error::Response(ResponseError::Cancelled));
        assert_eq!(reason, CapabilityFailureReason::Canceled);

        let (reason, _) =
            map_ashpd_error(Error::Portal(PortalError::NotAllowed("denied".to_string())));
        assert_eq!(reason, CapabilityFailureReason::PermissionDenied);

        let portal_name =
            OwnedInterfaceName::try_from("org.freedesktop.portal.Screenshot").unwrap();
        let (reason, _) = map_ashpd_error(Error::PortalNotFound(portal_name));
        assert_eq!(reason, CapabilityFailureReason::Unavailable);
    }

    #[cfg(all(feature = "linux-capture", target_os = "linux"))]
    #[test]
    fn wayland_file_uri_converts_to_image_data_url() {
        let path = std::env::temp_dir().join(format!(
            "psychevo-wayland-portal-screenshot-{}.png",
            std::process::id()
        ));
        std::fs::write(&path, [137, 80, 78, 71]).expect("write fake png bytes");

        let uri = format!("file://{}", path.display());
        let data_url = portal_screenshot_uri_to_data_url(&uri).expect("file uri converts");
        let _ = std::fs::remove_file(path);

        assert!(data_url.starts_with("data:image/png;base64,"));
    }
}
