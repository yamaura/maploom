#![doc = include_str!("../README.md")]

mod projection;
mod style;
use style::{CompiledLayer, CompiledPaint, compile_layers, set_fill_rgb, set_stroke_rgb};
#[cfg(test)]
mod tests;

use std::{borrow::Cow, error::Error, fmt};

use fast_mvt::{MvtError, MvtReaderRef, MvtTile};
use geo_types::{Coord, Geometry, LineString, Polygon};
pub use maploom_core::{
    FillLayer, FillLayout, FillPaint, Filter, Layer, LineCap, LineJoin, LineLayer, LineLayout,
    LinePaint, Number, Rgb, Visibility,
};
use pdf_writer::{Chunk, Content, Name, Rect, Ref, writers::ExtGraphicsState};
use projection::{WEB_MERCATOR_MAX_LATITUDE, latitude_to_world_y, longitude_to_world_x};

#[derive(Clone, Copy, Debug, PartialEq)]
/// Geographic bounds in degrees. West must precede east, and south precede north.
pub struct LngLatBounds {
    pub west: f64,
    pub south: f64,
    pub east: f64,
    pub north: f64,
}

impl LngLatBounds {
    /// The full normalized Web Mercator extent.
    pub fn world() -> Self {
        Self {
            west: -180.0,
            south: -WEB_MERCATOR_MAX_LATITUDE,
            east: 180.0,
            north: WEB_MERCATOR_MAX_LATITUDE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
/// Local drawing dimensions in points and the geographic viewport to fit inside them.
pub struct MapViewport {
    pub width_pt: f32,
    pub height_pt: f32,
    pub bounds: LngLatBounds,
    pub zoom: f64,
}

impl MapViewport {
    pub fn new(width_pt: f32, height_pt: f32, bounds: LngLatBounds, zoom: f64) -> Self {
        Self {
            width_pt,
            height_pt,
            bounds,
            zoom,
        }
    }
}

/// A Form XObject already written to a PDF chunk. It can be placed repeatedly.
///
/// The handle does not own the chunk. Its reference is valid only in the document
/// to which that chunk belongs. Keep it in sync if you renumber chunk references.
#[derive(Clone, Copy, Debug)]
pub struct MapXObject {
    id: Ref,
    width_pt: f32,
    height_pt: f32,
}

impl MapXObject {
    /// Register this reference in the containing page/form's XObject resources.
    pub fn id(&self) -> Ref {
        self.id
    }

    /// Local form bounds, before placement in the document.
    pub fn bounds(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width_pt, self.height_pt)
    }

    /// Place the map in `area`, preserving aspect ratio and centering it.
    ///
    /// `name` must be registered with [`Self::id`] in the containing content's
    /// XObject resources. Coordinates use the current PDF user space (normally
    /// points, with the origin at the bottom left). Existing transforms and
    /// clipping apply. The form's bounding box clips the map, and the caller's
    /// graphics state is preserved. Call between paths/text objects, not inside
    /// an unfinished path or `BT`/`ET` block.
    ///
    /// Line widths scale along with the form. For physical-size styling, render
    /// a viewport with the intended placement dimensions. Invalid areas return
    /// an error without writing any content.
    pub fn draw(
        &self,
        content: &mut Content,
        name: Name<'_>,
        area: Rect,
    ) -> Result<(), PdfRenderError> {
        let width = f64::from(area.x2) - f64::from(area.x1);
        let height = f64::from(area.y2) - f64::from(area.y1);
        if ![area.x1, area.y1, area.x2, area.y2]
            .iter()
            .all(|n| n.is_finite())
            || width <= 0.0
            || height <= 0.0
        {
            return Err(PdfRenderError::InvalidArea { area });
        }
        let scale = (width / f64::from(self.width_pt)).min(height / f64::from(self.height_pt));
        let x = f64::from(area.x1) + (width - f64::from(self.width_pt) * scale) / 2.0;
        let y = f64::from(area.y1) + (height - f64::from(self.height_pt) * scale) / 2.0;
        let matrix = [scale as f32, 0.0, 0.0, scale as f32, x as f32, y as f32];
        if !matrix.iter().all(|n| n.is_finite()) || matrix[0] <= 0.0 {
            return Err(PdfRenderError::InvalidArea { area });
        }
        content.save_state();
        content.transform(matrix);
        content.x_object(name);
        content.restore_state();
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
/// Uncompressed vector tile bytes with their XYZ tile coordinates.
pub struct MvtTileInput<'a> {
    /// Optional source identifier, matched against `Layer::source`.
    pub source: Option<Cow<'a, str>>,
    pub z: u8,
    pub x: u32,
    pub y: u32,
    pub bytes: Cow<'a, [u8]>,
}

impl<'a> MvtTileInput<'a> {
    pub fn new(z: u8, x: u32, y: u32, bytes: impl Into<Cow<'a, [u8]>>) -> Self {
        Self {
            source: None,
            z,
            x,
            y,
            bytes: bytes.into(),
        }
    }

    /// Associate this tile with a source without renaming its MVT layers.
    pub fn with_source(mut self, source: impl Into<Cow<'a, str>>) -> Self {
        self.source = Some(source.into());
        self
    }
}

/// Owned, decoded vector tiles reusable across layers, viewports and PDF layers.
/// Input bytes need not be retained after decoding. Tiles retain their input order.
#[derive(Debug, Default)]
pub struct TileSet {
    tiles: Vec<DecodedTile>,
}

impl TileSet {
    /// Validate XYZ coordinates and decode every tile, preserving source names.
    pub fn decode(inputs: &[MvtTileInput<'_>]) -> Result<Self, PdfRenderError> {
        Ok(Self {
            tiles: decode_tiles(inputs)?,
        })
    }
}

#[derive(Debug)]
/// Invalid rendering input or failure to decode a tile.
pub enum PdfRenderError {
    InvalidViewportSize { width_pt: f32, height_pt: f32 },
    InvalidArea { area: Rect },
    InvalidBounds,
    InvalidZoom,
    InvalidTileCoordinates { index: usize },
    DecodeTile { index: usize, error: MvtError },
    InvalidLayer { index: usize, message: String },
}

impl fmt::Display for PdfRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidViewportSize {
                width_pt,
                height_pt,
            } => write!(f, "invalid map viewport size: {width_pt} x {height_pt} pt"),
            Self::InvalidArea { area } => write!(f, "invalid map placement area: {area:?}"),
            Self::InvalidBounds => write!(f, "invalid Web Mercator map bounds"),
            Self::InvalidZoom => write!(f, "map zoom must be finite and nonnegative"),
            Self::InvalidTileCoordinates { index } => {
                write!(f, "invalid XYZ coordinates for tile {index}")
            }
            Self::DecodeTile { index, error } => {
                write!(f, "failed to decode MVT tile {index}: {error}")
            }
            Self::InvalidLayer { index, message } => {
                write!(f, "invalid render layer {index}: {message}")
            }
        }
    }
}

impl Error for PdfRenderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DecodeTile { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Write a reusable vector map as one Form XObject in a caller-owned PDF chunk.
///
/// `id` must be unused in the destination. No catalog, pages or page resources
/// are created. All map resources are local to the form; no additional object
/// IDs are needed. The chunk is unchanged if rendering input is invalid.
///
/// Placement through [`MapXObject::draw`] requires registration in XObject resources.
pub fn write_map(
    chunk: &mut Chunk,
    id: Ref,
    view: MapViewport,
    layers: &[Layer],
    tiles: &TileSet,
) -> Result<MapXObject, PdfRenderError> {
    let viewport = Viewport::new(view)?;
    let compiled_layers = compile_layers(layers, view.zoom)?;
    let ext_g_states = ext_g_states(&compiled_layers);
    let ext_g_state_names = ext_g_state_names(ext_g_states.len());

    let mut content = Content::new();
    for compiled in &compiled_layers {
        let alpha_index = ext_g_states
            .iter()
            .position(|alpha| *alpha == compiled.style.opacity())
            .unwrap_or(0);
        for tile in &tiles.tiles {
            if compiled
                .source
                .as_deref()
                .is_some_and(|source| tile.source.as_deref() != Some(source))
            {
                continue;
            }
            let Some(layer) = tile
                .tile
                .layers
                .iter()
                .find(|layer| layer.name == compiled.layer_name)
            else {
                continue;
            };
            let transform = TileTransform::new(tile.z, tile.x, tile.y, layer.extent.get());
            for feature in &layer.features {
                if !compiled.matches(feature) {
                    continue;
                }
                match &compiled.style {
                    CompiledPaint::Fill(style) => {
                        if !matches!(
                            feature.geometry,
                            Geometry::Polygon(_) | Geometry::MultiPolygon(_)
                        ) {
                            continue;
                        }
                        content.set_parameters(Name(&ext_g_state_names[alpha_index]));
                        set_fill_rgb(&mut content, style.color);
                        append_fill_geometry(&mut content, &feature.geometry, transform, viewport);
                        content.fill_even_odd();
                    }
                    CompiledPaint::Line(style) => {
                        if style.width <= 0.0 {
                            continue;
                        }
                        content.set_parameters(Name(&ext_g_state_names[alpha_index]));
                        set_stroke_rgb(&mut content, style.color);
                        content.set_line_width(style.width);
                        content.set_line_cap(style.cap);
                        content.set_line_join(style.join);
                        match &style.dasharray {
                            Some(dasharray) => {
                                content.set_dash_pattern(dasharray.iter().copied(), 0.0);
                            }
                            None => {
                                content.set_dash_pattern([], 0.0);
                            }
                        }
                        append_line_geometry(&mut content, &feature.geometry, transform, viewport);
                        content.stroke();
                    }
                }
            }
        }
    }

    let content = content.finish();
    {
        let mut form = chunk.form_xobject(id, &content);
        form.bbox(Rect::new(0.0, 0.0, view.width_pt, view.height_pt));
        let mut resources = form.resources();
        let mut states = resources.ext_g_states();
        for (index, alpha) in ext_g_states.iter().enumerate() {
            states
                .insert(Name(&ext_g_state_names[index]))
                .start::<ExtGraphicsState<'_>>()
                .stroking_alpha(*alpha)
                .non_stroking_alpha(*alpha);
        }
    }
    chunk.merge_limits(content.limits());
    Ok(MapXObject {
        id,
        width_pt: view.width_pt,
        height_pt: view.height_pt,
    })
}

fn decode_tiles(tiles: &[MvtTileInput<'_>]) -> Result<Vec<DecodedTile>, PdfRenderError> {
    tiles
        .iter()
        .enumerate()
        .map(|(index, tile)| {
            if tile.z > 32
                || u64::from(tile.x) >= (1_u64 << tile.z)
                || u64::from(tile.y) >= (1_u64 << tile.z)
            {
                return Err(PdfRenderError::InvalidTileCoordinates { index });
            }
            MvtReaderRef::new(tile.bytes.as_ref())
                .and_then(|reader| reader.to_tile())
                .map(|decoded| DecodedTile {
                    source: tile.source.as_deref().map(str::to_owned),
                    z: tile.z,
                    x: tile.x,
                    y: tile.y,
                    tile: decoded,
                })
                .map_err(|error| PdfRenderError::DecodeTile { index, error })
        })
        .collect()
}

fn ext_g_states(layers: &[CompiledLayer]) -> Vec<f32> {
    let mut alphas = Vec::new();
    for layer in layers {
        let alpha = layer.style.opacity();
        if !alphas.contains(&alpha) {
            alphas.push(alpha);
        }
    }
    if alphas.is_empty() {
        alphas.push(1.0);
    }
    alphas
}

fn ext_g_state_names(len: usize) -> Vec<Vec<u8>> {
    (0..len)
        .map(|index| format!("GS{index}").into_bytes())
        .collect()
}

#[derive(Debug)]
struct DecodedTile {
    source: Option<String>,
    z: u8,
    x: u32,
    y: u32,
    tile: MvtTile,
}

#[derive(Clone, Copy)]
struct TileTransform {
    z: u8,
    x: u32,
    y: u32,
    extent: f64,
}

impl TileTransform {
    fn new(z: u8, x: u32, y: u32, extent: u32) -> Self {
        Self {
            z,
            x,
            y,
            extent: f64::from(extent),
        }
    }

    fn world(self, coord: Coord<i32>) -> WorldPoint {
        let scale = 2.0_f64.powi(i32::from(self.z));
        WorldPoint {
            x: (f64::from(self.x) + f64::from(coord.x) / self.extent) / scale,
            y: (f64::from(self.y) + f64::from(coord.y) / self.extent) / scale,
        }
    }
}

#[derive(Clone, Copy)]
struct Viewport {
    left: f64,
    top: f64,
    scale_pt_per_world_unit: f64,
    offset_x_pt: f64,
    offset_y_pt: f64,
    height_pt: f32,
}

impl Viewport {
    fn new(view: MapViewport) -> Result<Self, PdfRenderError> {
        if !view.zoom.is_finite() || view.zoom < 0.0 {
            return Err(PdfRenderError::InvalidZoom);
        }
        let bounds = view.bounds;
        if ![bounds.west, bounds.east, bounds.south, bounds.north]
            .iter()
            .all(|v| v.is_finite())
            || bounds.west < -180.0
            || bounds.east > 180.0
            || bounds.south < -WEB_MERCATOR_MAX_LATITUDE
            || bounds.north > WEB_MERCATOR_MAX_LATITUDE
        {
            return Err(PdfRenderError::InvalidBounds);
        }
        if view.width_pt <= 0.0
            || view.height_pt <= 0.0
            || !view.width_pt.is_finite()
            || !view.height_pt.is_finite()
        {
            return Err(PdfRenderError::InvalidViewportSize {
                width_pt: view.width_pt,
                height_pt: view.height_pt,
            });
        }

        let west = longitude_to_world_x(view.bounds.west);
        let east = longitude_to_world_x(view.bounds.east);
        let north = latitude_to_world_y(view.bounds.north);
        let south = latitude_to_world_y(view.bounds.south);
        let width = east - west;
        let height = south - north;
        if width <= 0.0 || height <= 0.0 || !width.is_finite() || !height.is_finite() {
            return Err(PdfRenderError::InvalidBounds);
        }
        let scale_pt_per_world_unit =
            (f64::from(view.width_pt) / width).min(f64::from(view.height_pt) / height);
        let rendered_width_pt = width * scale_pt_per_world_unit;
        let rendered_height_pt = height * scale_pt_per_world_unit;

        Ok(Self {
            left: west,
            top: north,
            scale_pt_per_world_unit,
            offset_x_pt: (f64::from(view.width_pt) - rendered_width_pt) * 0.5,
            offset_y_pt: (f64::from(view.height_pt) - rendered_height_pt) * 0.5,
            height_pt: view.height_pt,
        })
    }

    fn point(self, world: WorldPoint) -> (f32, f32) {
        (
            (self.offset_x_pt + (world.x - self.left) * self.scale_pt_per_world_unit) as f32,
            (f64::from(self.height_pt)
                - self.offset_y_pt
                - (world.y - self.top) * self.scale_pt_per_world_unit) as f32,
        )
    }
}

#[derive(Clone, Copy)]
struct WorldPoint {
    x: f64,
    y: f64,
}

fn append_fill_geometry(
    content: &mut Content,
    geometry: &fast_mvt::MvtGeometry,
    tile: TileTransform,
    viewport: Viewport,
) {
    match geometry {
        Geometry::Polygon(polygon) => append_polygon(content, polygon, tile, viewport),
        Geometry::MultiPolygon(polygons) => {
            for polygon in polygons {
                append_polygon(content, polygon, tile, viewport);
            }
        }
        _ => {}
    }
}

fn append_line_geometry(
    content: &mut Content,
    geometry: &fast_mvt::MvtGeometry,
    tile: TileTransform,
    viewport: Viewport,
) {
    match geometry {
        Geometry::LineString(line) => append_line_string(content, line, false, tile, viewport),
        Geometry::MultiLineString(lines) => {
            for line in lines {
                append_line_string(content, line, false, tile, viewport);
            }
        }
        Geometry::Polygon(polygon) => append_polygon_outline(content, polygon, tile, viewport),
        Geometry::MultiPolygon(polygons) => {
            for polygon in polygons {
                append_polygon_outline(content, polygon, tile, viewport);
            }
        }
        _ => {}
    }
}

fn append_polygon(
    content: &mut Content,
    polygon: &Polygon<i32>,
    tile: TileTransform,
    viewport: Viewport,
) {
    append_line_string(content, polygon.exterior(), true, tile, viewport);
    for ring in polygon.interiors() {
        append_line_string(content, ring, true, tile, viewport);
    }
}

fn append_polygon_outline(
    content: &mut Content,
    polygon: &Polygon<i32>,
    tile: TileTransform,
    viewport: Viewport,
) {
    append_line_string(content, polygon.exterior(), true, tile, viewport);
    for ring in polygon.interiors() {
        append_line_string(content, ring, true, tile, viewport);
    }
}

fn append_line_string(
    content: &mut Content,
    line: &LineString<i32>,
    close: bool,
    tile: TileTransform,
    viewport: Viewport,
) {
    let mut coords = line.coords();
    let Some(first) = coords.next() else {
        return;
    };
    let (x, y) = viewport.point(tile.world(*first));
    content.move_to(x, y);
    for coord in coords {
        let (x, y) = viewport.point(tile.world(*coord));
        content.line_to(x, y);
    }
    if close {
        content.close_path();
    }
}
