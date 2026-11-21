#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod projection;
mod style;
#[cfg(feature = "fonts")]
mod text;
#[cfg(feature = "fonts")]
use maploom_pdf_fonts::{self as fonts, FontResources, TextError};
#[cfg(feature = "fonts")]
#[cfg_attr(docsrs, doc(cfg(feature = "fonts")))]
#[doc(inline)]
pub use maploom_pdf_fonts::{FontCatalog, FontError};
pub use style::validate_layers;
use style::{CompiledLayer, CompiledPaint, compile_layers, set_fill_rgb, set_stroke_rgb};

use std::borrow::Cow;

use fast_mvt::{MvtError, MvtReaderRef, MvtTile};
use geo_types::{Coord, Geometry, LineString, Polygon};
pub use maploom_core::{
    Expression, FillLayer, FillLayout, FillPaint, Filter, Layer, LineCap, LineJoin, LineLayer,
    LineLayout, LinePaint, Number, Rgb, TextAnchor, TextField, TextLayer, TextLayout, TextPaint,
    Visibility,
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
    /// Reference to register in the containing page/form's XObject resources.
    pub fn id(&self) -> Ref {
        self.id
    }

    /// Local form bounds, before placement in the document.
    pub fn bounds(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width_pt, self.height_pt)
    }

    /// Place the map in `area`, preserving aspect ratio and centering it.
    ///
    /// Register `name` with [`Self::id`] in the containing XObject resources.
    /// `area` uses the current PDF user space; existing transforms and clipping
    /// apply. Drawing preserves graphics state and clips to the form's bounds.
    /// Call outside unfinished paths and `BT`/`ET` blocks.
    ///
    /// Line widths scale with the form. For physical-size styling, match the
    /// viewport to the placement dimensions. Invalid areas leave content unchanged.
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

    fn world_copies(&self, count: u8) -> impl Iterator<Item = (&DecodedTile, u8)> {
        self.tiles
            .iter()
            .flat_map(move |tile| (0..count).map(move |offset| (tile, offset)))
    }
}

/// Invalid rendering input or failure to decode a tile.
#[derive(Debug, thiserror::Error)]
pub enum PdfRenderError {
    #[cfg(feature = "fonts")]
    #[cfg_attr(docsrs, doc(cfg(feature = "fonts")))]
    #[error("{0}")]
    Font(#[source] FontError),
    #[error("invalid text in layer {layer:?}, feature {feature:?}: {message}")]
    Text {
        layer: String,
        feature: Option<u64>,
        message: String,
    },
    #[error("invalid map viewport size: {width_pt} x {height_pt} pt")]
    InvalidViewportSize { width_pt: f32, height_pt: f32 },
    #[error("invalid map placement area: {area:?}")]
    InvalidArea { area: Rect },
    #[error("invalid Web Mercator map bounds")]
    InvalidBounds,
    #[error("map zoom must be finite and nonnegative")]
    InvalidZoom,
    #[error("invalid XYZ coordinates for tile {index}")]
    InvalidTileCoordinates { index: usize },
    #[error("failed to decode MVT tile {index}: {error}")]
    DecodeTile {
        index: usize,
        #[source]
        error: MvtError,
    },
    #[error("invalid render layer {index}: {message}")]
    InvalidLayer { index: usize, message: String },
}

/// Write a reusable vector map as one Form XObject in a caller-owned PDF chunk.
///
/// `id` must be unused. The caller owns the catalog, pages and page resources.
/// Rendering needs no additional object IDs. Invalid input leaves the chunk unchanged.
///
/// Placement through [`MapXObject::draw`] requires registration in XObject resources.
pub fn write_map(
    chunk: &mut Chunk,
    id: Ref,
    view: MapViewport,
    layers: &[Layer],
    tiles: &TileSet,
) -> Result<MapXObject, PdfRenderError> {
    #[cfg(feature = "fonts")]
    let fonts = FontCatalog::new();
    let mut next = id;
    #[cfg(feature = "fonts")]
    let mut writer = PdfMapWriter::new(chunk, &mut next, &fonts);
    #[cfg(not(feature = "fonts"))]
    let mut writer = PdfMapWriter::new(chunk, &mut next);
    writer.write_map(view, layers, tiles)
}

/// A document-scoped map writer that shares embedded fonts between forms.
///
/// The caller owns the chunk, increasing reference cursor and font data. Keep
/// using the same cursor for subsequent PDF objects. [`Self::finish`] or dropping the
/// writer finalizes shared fonts and releases the document borrow.
pub struct PdfMapWriter<'document, 'fonts, 'data> {
    chunk: &'document mut Chunk,
    next: &'document mut Ref,
    #[cfg(feature = "fonts")]
    fonts: &'fonts FontCatalog<'data>,
    #[cfg(feature = "fonts")]
    font_resources: FontResources<'fonts, 'data>,
    #[cfg(not(feature = "fonts"))]
    _fonts: std::marker::PhantomData<(&'fonts (), &'data ())>,
}

impl<'document, 'fonts, 'data> PdfMapWriter<'document, 'fonts, 'data> {
    #[cfg(feature = "fonts")]
    #[cfg_attr(docsrs, doc(cfg(feature = "fonts")))]
    pub fn new(
        chunk: &'document mut Chunk,
        next: &'document mut Ref,
        fonts: &'fonts FontCatalog<'data>,
    ) -> Self {
        Self {
            chunk,
            next,
            fonts,
            font_resources: FontResources::new(fonts),
        }
    }

    /// Create a geometry-only writer when the fonts feature is disabled.
    #[cfg(not(feature = "fonts"))]
    #[cfg_attr(docsrs, doc(cfg(not(feature = "fonts"))))]
    pub fn new(chunk: &'document mut Chunk, next: &'document mut Ref) -> Self {
        Self {
            chunk,
            next,
            _fonts: std::marker::PhantomData,
        }
    }

    /// Write a form. Errors leave the chunk and reference cursor unchanged.
    pub fn write_map(
        &mut self,
        view: MapViewport,
        layers: &[Layer],
        tiles: &TileSet,
    ) -> Result<MapXObject, PdfRenderError> {
        self.write_map_inner(view, layers, tiles, false)
    }

    /// Write a horizontally repeating map from canonical XYZ tiles.
    /// Bounds use continuous longitudes (e.g. 170..190), spanning at most 360°.
    /// Text placement and collisions are shared across the antimeridian.
    pub fn write_wrapped_map(
        &mut self,
        view: MapViewport,
        layers: &[Layer],
        tiles: &TileSet,
    ) -> Result<MapXObject, PdfRenderError> {
        self.write_map_inner(view, layers, tiles, true)
    }

    fn write_map_inner(
        &mut self,
        view: MapViewport,
        layers: &[Layer],
        tiles: &TileSet,
        wrap: bool,
    ) -> Result<MapXObject, PdfRenderError> {
        let viewport = Viewport::new(view, wrap)?;
        let compiled_layers = compile_layers(layers, view.zoom)?;
        #[cfg(feature = "fonts")]
        let mut labels = prepare_labels(&compiled_layers, tiles, viewport, view, self.fonts)?;
        #[cfg(feature = "fonts")]
        let prepared = self
            .font_resources
            .prepare_form(labels.iter_mut().flatten(), self.next)
            .map_err(PdfRenderError::Font)?;
        #[cfg(feature = "fonts")]
        let id = prepared.id;
        #[cfg(not(feature = "fonts"))]
        let id = self.next.bump();
        let ext_g_states = ext_g_states(&compiled_layers);
        let ext_g_state_names = ext_g_state_names(ext_g_states.len());

        let mut content = Content::new();
        #[cfg(feature = "fonts")]
        let rendered_layers = compiled_layers.iter().zip(&labels);
        #[cfg(not(feature = "fonts"))]
        let rendered_layers = compiled_layers.iter().map(|compiled| (compiled, ()));
        for (compiled, _labels) in rendered_layers {
            let alpha_index = ext_g_states
                .iter()
                .position(|alpha| *alpha == compiled.style.opacity())
                .unwrap_or(0);
            #[cfg(feature = "fonts")]
            if let CompiledPaint::Text(style) = &compiled.style {
                content.set_parameters(Name(&ext_g_state_names[alpha_index]));
                for label in _labels {
                    fonts::draw(&mut content, label, style, self.fonts);
                }
                continue;
            }
            for (tile, world_offset) in tiles.world_copies(viewport.world_copies) {
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
                let transform =
                    TileTransform::new(tile.z, tile.x, tile.y, layer.extent.get(), world_offset);
                // Tile buffers must not darken adjacent translucent geometry.
                let scale = 2.0_f64.powi(i32::from(tile.z));
                let (left, top) = viewport.point(WorldPoint {
                    x: f64::from(tile.x) / scale + f64::from(world_offset),
                    y: f64::from(tile.y) / scale,
                });
                let (right, bottom) = viewport.point(WorldPoint {
                    x: (f64::from(tile.x) + 1.0) / scale + f64::from(world_offset),
                    y: (f64::from(tile.y) + 1.0) / scale,
                });
                content.save_state();
                content.rect(left, bottom, right - left, top - bottom);
                content.clip_nonzero();
                content.end_path();
                for feature in &layer.features {
                    if !compiled.matches(feature) {
                        continue;
                    }
                    match &compiled.style {
                        #[cfg(feature = "fonts")]
                        CompiledPaint::Text(_) => {
                            unreachable!("text is rendered globally, without tile clipping")
                        }
                        CompiledPaint::Fill(style) => {
                            if !matches!(
                                feature.geometry,
                                Geometry::Polygon(_) | Geometry::MultiPolygon(_)
                            ) {
                                continue;
                            }
                            content.set_parameters(Name(&ext_g_state_names[alpha_index]));
                            set_fill_rgb(&mut content, style.color);
                            append_fill_geometry(
                                &mut content,
                                &feature.geometry,
                                transform,
                                viewport,
                            );
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
                            append_line_geometry(
                                &mut content,
                                &feature.geometry,
                                transform,
                                viewport,
                            );
                            content.stroke();
                        }
                    }
                }
                content.restore_state();
            }
        }

        let content = content.finish();
        {
            let mut form = self.chunk.form_xobject(id, &content);
            form.bbox(Rect::new(0.0, 0.0, view.width_pt, view.height_pt));
            let mut resources = form.resources();
            #[cfg(feature = "fonts")]
            {
                let mut fonts = resources.fonts();
                for (name, reference) in prepared.fonts {
                    fonts.pair(Name(name.as_bytes()), reference);
                }
            }
            let mut states = resources.ext_g_states();
            for (index, alpha) in ext_g_states.iter().enumerate() {
                states
                    .insert(Name(&ext_g_state_names[index]))
                    .start::<ExtGraphicsState<'_>>()
                    .stroking_alpha(*alpha)
                    .non_stroking_alpha(*alpha);
            }
        }
        self.chunk.merge_limits(content.limits());
        Ok(MapXObject {
            id,
            width_pt: view.width_pt,
            height_pt: view.height_pt,
        })
    }

    /// Finalize shared font resources and release the document borrow.
    /// Dropping the writer performs the same finalization automatically.
    pub fn finish(self) {}
}

#[cfg(feature = "fonts")]
#[cfg_attr(docsrs, doc(cfg(feature = "fonts")))]
impl Drop for PdfMapWriter<'_, '_, '_> {
    fn drop(&mut self) {
        self.font_resources.finish(self.chunk);
    }
}

#[cfg(feature = "fonts")]
fn prepare_labels(
    layers: &[CompiledLayer],
    tiles: &TileSet,
    viewport: Viewport,
    view: MapViewport,
    fonts: &FontCatalog<'_>,
) -> Result<Vec<Vec<fonts::Label>>, PdfRenderError> {
    let mut collisions = text::Collisions::new(view.width_pt, view.height_pt);
    let mut result = Vec::with_capacity(layers.len());
    for compiled in layers {
        let mut labels = Vec::new();
        if let CompiledPaint::Text(style) = &compiled.style
            && style.size > 0.0
            && style.opacity > 0.0
        {
            for (tile, world_offset) in tiles.world_copies(viewport.world_copies) {
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
                let transform =
                    TileTransform::new(tile.z, tile.x, tile.y, layer.extent.get(), world_offset);
                for feature in &layer.features {
                    if !compiled.matches(feature) {
                        continue;
                    }
                    let points: Vec<_> = match &feature.geometry {
                        Geometry::Point(point) => vec![point.0],
                        Geometry::MultiPoint(points) => {
                            points.iter().map(|point| point.0).collect()
                        }
                        _ => continue,
                    };
                    let text = style
                        .layout
                        .field
                        .evaluate(&maploom_filter_fast_mvt::FastMvtFeature(feature))
                        .map_err(|error| PdfRenderError::Text {
                            layer: compiled.id.clone(),
                            feature: feature.id,
                            message: error.to_string(),
                        })?;
                    if text.trim().is_empty() {
                        continue;
                    }
                    for point in points {
                        // Anchor ownership prevents duplicate labels across tile buffers.
                        if point.x < 0
                            || point.y < 0
                            || f64::from(point.x) >= transform.extent
                            || f64::from(point.y) >= transform.extent
                        {
                            continue;
                        }
                        let label = fonts::layout(
                            fonts,
                            style,
                            &text,
                            viewport.point(transform.world(point)),
                        )
                        .map_err(|error| match error {
                            TextError::Font(error) => PdfRenderError::Font(error),
                            TextError::CoordinateOverflow => PdfRenderError::Text {
                                layer: compiled.id.clone(),
                                feature: feature.id,
                                message: error.to_string(),
                            },
                        })?;
                        if !collisions.visible(label.bounds)
                            || (!style.layout.allow_overlap && collisions.overlaps(label.bounds))
                        {
                            continue;
                        }
                        if !style.layout.ignore_placement {
                            collisions.insert(label.bounds);
                        }
                        labels.push(label);
                    }
                }
            }
        }
        result.push(labels);
    }
    Ok(result)
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
    world_offset: u8,
}

impl TileTransform {
    fn new(z: u8, x: u32, y: u32, extent: u32, world_offset: u8) -> Self {
        Self {
            z,
            x,
            y,
            extent: f64::from(extent),
            world_offset,
        }
    }

    fn world(self, coord: Coord<i32>) -> WorldPoint {
        let scale = 2.0_f64.powi(i32::from(self.z));
        WorldPoint {
            x: (f64::from(self.x) + f64::from(coord.x) / self.extent) / scale
                + f64::from(self.world_offset),
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
    world_copies: u8,
}

impl Viewport {
    fn new(view: MapViewport, wrap: bool) -> Result<Self, PdfRenderError> {
        if !view.zoom.is_finite() || view.zoom < 0.0 {
            return Err(PdfRenderError::InvalidZoom);
        }
        let bounds = view.bounds;
        if ![bounds.west, bounds.east, bounds.south, bounds.north]
            .iter()
            .all(|v| v.is_finite())
            || (!wrap && (bounds.west < -180.0 || bounds.east > 180.0))
            || (wrap && bounds.east - bounds.west > 360.0 + 1e-7)
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

        let longitude_span = if wrap {
            (bounds.east - bounds.west).min(360.0)
        } else {
            bounds.east - bounds.west
        };
        let west = longitude_to_world_x(if wrap {
            (bounds.west + 180.0).rem_euclid(360.0) - 180.0
        } else {
            bounds.west
        });
        let north = latitude_to_world_y(view.bounds.north);
        let south = latitude_to_world_y(view.bounds.south);
        let width = if wrap {
            longitude_span / 360.0
        } else {
            longitude_to_world_x(bounds.east) - west
        };
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
            world_copies: if wrap && west + width > 1.0 { 2 } else { 1 },
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

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "fonts")]
    use fast_mvt::{DEFAULT_EXTENT, MvtFeature, MvtGeometry, MvtLayer, MvtTile, MvtValue};

    #[test]
    fn viewport_preserves_aspect_ratio() {
        let viewport = Viewport::new(
            MapViewport::new(300.0, 200.0, LngLatBounds::world(), 0.0),
            false,
        )
        .unwrap();

        assert_point_near(viewport.point(WorldPoint { x: 0.0, y: 0.0 }), (50.0, 200.0));
        assert_point_near(viewport.point(WorldPoint { x: 1.0, y: 1.0 }), (250.0, 0.0));
    }

    #[test]
    fn wrapped_viewport_validates_continuous_bounds() {
        for (west, east) in [
            (170.0, -170.0),
            (0.0, 361.0),
            (180.0, 180.0),
            (f64::NAN, 190.0),
        ] {
            let view = MapViewport::new(
                200.0,
                200.0,
                LngLatBounds {
                    west,
                    east,
                    ..LngLatBounds::world()
                },
                0.0,
            );
            assert!(matches!(
                Viewport::new(view, true),
                Err(PdfRenderError::InvalidBounds)
            ));
        }
        let view = MapViewport::new(
            200.0,
            200.0,
            LngLatBounds {
                west: 20.0,
                east: 380.0 + 1e-8,
                ..LngLatBounds::world()
            },
            0.0,
        );
        assert!(Viewport::new(view, true).is_ok());
    }

    fn assert_point_near(actual: (f32, f32), expected: (f32, f32)) {
        const EPSILON: f32 = 0.001;
        assert!(
            (actual.0 - expected.0).abs() <= EPSILON,
            "x mismatch: actual {}, expected {}",
            actual.0,
            expected.0
        );
        assert!(
            (actual.1 - expected.1).abs() <= EPSILON,
            "y mismatch: actual {}, expected {}",
            actual.1,
            expected.1
        );
    }

    #[cfg(feature = "fonts")]
    fn encode_tile(layers: Vec<MvtLayer>) -> Vec<u8> {
        MvtTile { layers }.encode().unwrap()
    }

    #[cfg(feature = "fonts")]
    fn label_tiles(points: &[(i32, i32, &str)]) -> TileSet {
        let features = points
            .iter()
            .map(|&(x, y, name)| {
                let mut feature = MvtFeature::new(MvtGeometry::Point(geo_types::Point::new(x, y)));
                feature.add_tag("name", MvtValue::from(name));
                feature
            })
            .collect();
        let bytes = encode_tile(vec![MvtLayer {
            name: "places".into(),
            extent: DEFAULT_EXTENT,
            features,
        }]);
        TileSet::decode(&[MvtTileInput::new(0, 0, 0, bytes)]).unwrap()
    }

    #[cfg(feature = "fonts")]
    fn label_layer() -> TextLayer {
        let mut layer = TextLayer::new("labels", "places", TextField::property("name"));
        layer.layout.fonts = vec!["Latin".into()];
        layer
    }

    #[test]
    #[cfg(feature = "fonts")]
    fn point_ownership_collision_flags_and_layer_priority_are_deterministic() {
        let mut fonts = FontCatalog::new();
        fonts.register("Latin", font_test_data::AHEM).unwrap();
        let tiles = label_tiles(&[
            (2048, 2048, "A"),
            (2048, 2048, "B"),
            (-1, 2048, "Buffer"),
            (4096, 2048, "Next"),
        ]);
        let view = MapViewport::new(200.0, 200.0, LngLatBounds::world(), 0.0);
        let viewport = Viewport::new(view, false).unwrap();
        let mut first = label_layer();
        let mut second = first.clone();
        second.id = "second".into();
        let labels = |first: TextLayer, second: TextLayer| {
            let compiled = compile_layers(&[Layer::Text(first), Layer::Text(second)], 0.0).unwrap();
            prepare_labels(&compiled, &tiles, viewport, view, &fonts)
                .unwrap()
                .iter()
                .map(Vec::len)
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(first.clone(), second.clone()), [1, 0]);
        first.layout.allow_overlap = true;
        assert_eq!(labels(first.clone(), second.clone()), [2, 0]);
        first.layout.ignore_placement = true;
        assert_eq!(labels(first, second.clone()), [2, 1]);
        second.layout.allow_overlap = true;
        assert_eq!(labels(label_layer(), second), [1, 2]);
    }
}
