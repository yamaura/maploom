use fast_mvt::{DEFAULT_EXTENT, MvtFeature, MvtGeometry, MvtLayer, MvtTile};
use geo_types::{line_string, polygon};
use maploom_pdf::{FillLayer, Layer, LineCap, LineLayer, MvtTileInput, Rgb, TileSet};

pub fn sample_data() -> Result<(Vec<Layer>, TileSet), Box<dyn std::error::Error>> {
    let bytes = MvtTile {
        layers: vec![
            MvtLayer {
                name: "land".into(),
                extent: DEFAULT_EXTENT,
                features: vec![MvtFeature::new(MvtGeometry::Polygon(polygon![
                    (x: 512, y: 512), (x: 3584, y: 512),
                    (x: 3584, y: 3584), (x: 512, y: 3584), (x: 512, y: 512),
                ]))],
            },
            MvtLayer {
                name: "roads".into(),
                extent: DEFAULT_EXTENT,
                features: vec![MvtFeature::new(MvtGeometry::LineString(line_string![
                    (x: 512, y: 1024), (x: 2048, y: 2560), (x: 3584, y: 1536),
                ]))],
            },
        ],
    }
    .encode()?;
    let mut land = FillLayer::new("land-fill", "land");
    land.source = Some("basemap".into());
    land.paint.color = Rgb::new(216, 234, 211);
    let mut roads = LineLayer::new("roads", "roads");
    roads.source = Some("basemap".into());
    roads.paint.color = Rgb::new(51, 102, 153);
    roads.paint.width = 3.0.into();
    roads.layout.cap = LineCap::Round;
    let layers = vec![Layer::Fill(land), Layer::Line(roads)];
    let tiles = TileSet::decode(&[MvtTileInput::new(0, 0, 0, bytes).with_source("basemap")])?;
    Ok((layers, tiles))
}
