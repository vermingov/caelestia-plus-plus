//! The compositor's side of the background: a connection of its own, and on
//! every screen a surface under everything else.

use wayland_client::globals::GlobalListContents;
use wayland_client::protocol::{wl_compositor, wl_output, wl_region, wl_registry, wl_surface};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::wp::fractional_scale::v1::client::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

use super::paint::Canvas;

/// The name Quickshell's background goes by, so that a compositor rule
/// written for that one holds for this one.
const NAMESPACE: &str = "caelestia-background";

/// The first version of `wl_output` that says what the output is called.
const NAMED_OUTPUTS: u32 = 4;

/// A surface under everything on one screen.
pub struct Sheet {
    /// Before the surface it draws on, which is the order they are dropped
    /// in: a swapchain must not outlive its surface.
    pub canvas: Option<Canvas>,
    /// The picture that has been hung on it, or that could not be.
    pub hung: Option<std::path::PathBuf>,
    pub surface: wl_surface::WlSurface,
    layer: zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
    viewport: wp_viewport::WpViewport,
    fraction: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    /// As the compositor last said, in its own units, and whether anything
    /// has been done about it yet.
    pub size: (u32, u32),
    pub resized: bool,
    /// How many of the screen's pixels one of those units is, in 120ths,
    /// where the compositor says so to a surface rather than of an output.
    scale: Option<u32>,
}

impl Sheet {
    /// The size in the screen's own pixels, on an output that says it is
    /// `output_scale` to the unit where the surface has been told no better.
    pub fn pixels(&self, output_scale: i32) -> (u32, u32) {
        let scale = self.scale.unwrap_or(output_scale.max(1) as u32 * 120);
        // To the nearest, halves upwards, which is how the protocol has a
        // client round.
        let scaled = |length: u32| (length * scale + 60) / 120;
        (scaled(self.size.0), scaled(self.size.1))
    }

    /// Has the compositor stretch whatever is drawn to the size it gave, and
    /// tells it nothing under this needs drawing.
    pub fn fill(&self, compositor: &wl_compositor::WlCompositor, queue: &QueueHandle<Desk>) {
        let (width, height) = (self.size.0 as i32, self.size.1 as i32);
        self.viewport.set_destination(width, height);
        let all = compositor.create_region(queue, ());
        all.add(0, 0, width, height);
        self.surface.set_opaque_region(Some(&all));
        all.destroy();
    }
}

impl Drop for Sheet {
    fn drop(&mut self) {
        self.canvas = None;
        if let Some(fraction) = &self.fraction {
            fraction.destroy();
        }
        self.viewport.destroy();
        self.layer.destroy();
        self.surface.destroy();
    }
}

pub struct Screen {
    /// What the registry calls the output, which is how its going is said.
    global: u32,
    output: wl_output::WlOutput,
    /// What the compositor calls it: "eDP-1".
    pub name: String,
    /// Whole pixels to the unit, as an output says it.
    pub scale: i32,
    pub sheet: Option<Sheet>,
}

impl Drop for Screen {
    fn drop(&mut self) {
        // The sheet first: it was made for this output.
        self.sheet = None;
        self.output.release();
    }
}

pub struct Desk {
    pub compositor: wl_compositor::WlCompositor,
    layers: zwlr_layer_shell_v1::ZwlrLayerShellV1,
    viewporter: wp_viewporter::WpViewporter,
    /// Not every compositor has one, and one without it scales by whole
    /// numbers, which its outputs say.
    fractions: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    pub screens: Vec<Screen>,
}

impl Desk {
    pub fn new(
        compositor: wl_compositor::WlCompositor,
        layers: zwlr_layer_shell_v1::ZwlrLayerShellV1,
        viewporter: wp_viewporter::WpViewporter,
        fractions: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    ) -> Desk {
        Desk { compositor, layers, viewporter, fractions, screens: Vec::new() }
    }

    /// What there is to draw on, on every screen that has been given a size.
    pub fn canvases(&mut self) -> impl Iterator<Item = &mut Canvas> {
        self.screens.iter_mut().filter_map(|screen| screen.sheet.as_mut()?.canvas.as_mut())
    }

    pub fn output_came(&mut self, registry: &wl_registry::WlRegistry, global: u32, version: u32, queue: &QueueHandle<Desk>) {
        if version < NAMED_OUTPUTS {
            return eprintln!("cae: the helix: an output that cannot say its name is left without one");
        }
        let output = registry.bind(global, NAMED_OUTPUTS, queue, global);
        self.screens.push(Screen { global, output, name: String::new(), scale: 1, sheet: None });
    }

    fn sheet(&self, output: &wl_output::WlOutput, global: u32, queue: &QueueHandle<Desk>) -> Sheet {
        let surface = self.compositor.create_surface(queue, ());
        let layer = self.layers.get_layer_surface(
            &surface,
            Some(output),
            zwlr_layer_shell_v1::Layer::Background,
            NAMESPACE.to_string(),
            queue,
            global,
        );
        let every_edge = zwlr_layer_surface_v1::Anchor::all();
        layer.set_anchor(every_edge);
        // Under the bar's strip as well: nothing is reserved against it.
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::None);
        // Nothing here to press, so a press on the desktop is the
        // compositor's, as it is on a desktop with nothing on it.
        let nowhere = self.compositor.create_region(queue, ());
        surface.set_input_region(Some(&nowhere));
        nowhere.destroy();
        let viewport = self.viewporter.get_viewport(&surface, queue, ());
        let fraction = self.fractions.as_ref().map(|fractions| fractions.get_fractional_scale(&surface, queue, global));
        // The first commit has no picture: it asks what size to be.
        surface.commit();
        Sheet { canvas: None, hung: None, surface, layer, viewport, fraction, size: (0, 0), resized: false, scale: None }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Desk {
    fn event(
        desk: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global { name, interface, version } if interface == wl_output::WlOutput::interface().name => {
                desk.output_came(registry, name, version, queue);
            }
            wl_registry::Event::GlobalRemove { name } => desk.screens.retain(|screen| screen.global != name),
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, u32> for Desk {
    fn event(desk: &mut Self, output: &wl_output::WlOutput, event: wl_output::Event, global: &u32, _: &Connection, queue: &QueueHandle<Self>) {
        let Some(at) = desk.screens.iter().position(|screen| screen.global == *global) else { return };
        match event {
            wl_output::Event::Name { name } => desk.screens[at].name = name,
            wl_output::Event::Scale { factor } => desk.screens[at].scale = factor,
            // Everything about the output has been said, its name included.
            wl_output::Event::Done if desk.screens[at].sheet.is_none() => {
                desk.screens[at].sheet = Some(desk.sheet(output, *global, queue));
            }
            _ => {}
        }
    }
}

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, u32> for Desk {
    fn event(
        desk: &mut Self,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(screen) = desk.screens.iter_mut().find(|screen| screen.global == *global) else { return };
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, width, height } => {
                layer.ack_configure(serial);
                if let Some(sheet) = &mut screen.sheet {
                    (sheet.size, sheet.resized) = ((width, height), true);
                }
            }
            zwlr_layer_surface_v1::Event::Closed => screen.sheet = None,
            _ => {}
        }
    }
}

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, u32> for Desk {
    fn event(
        desk: &mut Self,
        _: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let wp_fractional_scale_v1::Event::PreferredScale { scale } = event else { return };
        let sheet = desk.screens.iter_mut().find(|screen| screen.global == *global).and_then(|screen| screen.sheet.as_mut());
        if let Some(sheet) = sheet.filter(|sheet| sheet.scale != Some(scale)) {
            (sheet.scale, sheet.resized) = (Some(scale), true);
        }
    }
}

delegate_noop!(Desk: wl_compositor::WlCompositor);
delegate_noop!(Desk: wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1);
delegate_noop!(Desk: wl_region::WlRegion);
delegate_noop!(Desk: zwlr_layer_shell_v1::ZwlrLayerShellV1);
delegate_noop!(Desk: wp_viewporter::WpViewporter);
delegate_noop!(Desk: wp_viewport::WpViewport);
// Which outputs it is on, which is the one it was put on.
delegate_noop!(Desk: ignore wl_surface::WlSurface);
