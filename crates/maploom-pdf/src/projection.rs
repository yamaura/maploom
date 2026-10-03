pub(crate) const WEB_MERCATOR_MAX_LATITUDE: f64 = 85.051_128_779_806_6;

pub(crate) fn longitude_to_world_x(longitude: f64) -> f64 {
    (longitude + 180.0) / 360.0
}

pub(crate) fn latitude_to_world_y(latitude: f64) -> f64 {
    let latitude = latitude.clamp(-WEB_MERCATOR_MAX_LATITUDE, WEB_MERCATOR_MAX_LATITUDE);
    let sin = latitude.to_radians().sin();
    0.5 - ((1.0 + sin) / (1.0 - sin)).ln() / (4.0 * std::f64::consts::PI)
}
