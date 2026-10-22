//! What each terrio command looks like: the only hand-written part. Everything else
//! (the options themselves) comes from terrio's `--help`.

pub const RASTER_PAT: &str = "*.tif;*.tiff";
pub const VECTOR_PAT: &str = "*.gpkg;*.shp;*.geojson;*.fgb;*.parquet;*.gml;*.kml;*.gpx";
pub const CLOUD_PAT: &str = "*.las;*.laz";
pub const ANY_PAT: &str = "*.tif;*.tiff;*.jp2;*.gpkg;*.shp;*.geojson;*.fgb;*.parquet;*.gml;*.kml;*.gpx;*.las;*.laz;*.pmtiles;*.mbtiles;*.nc;*.h5";

pub const EXTENSIONS: &[&str] = &[
    ".tif", ".vrt", ".gpkg", ".fgb", ".parquet", ".geojson", ".shp", ".csv", ".kml", ".pmtiles",
    ".mbtiles", ".zarr", ".nc", ".jp2", ".png", ".jpg", ".laz", ".las", ".copc.laz", ".ply", ".e57",
    "/ept.json", "",
];

pub const DEM_MODES: &[&str] = &["hillshade", "slope", "aspect", "color-relief", "TRI", "TPI", "roughness"];

const COMPRESS: &[&str] = &["NONE", "LZW", "DEFLATE", "ZSTD", "PACKBITS", "JPEG", "WEBP", "PNG", "ZLIB", "GZIP"];
const RESAMPLING: &[&str] = &[
    "near", "bilinear", "cubic", "cubicspline", "lanczos", "average", "rms", "mode", "min", "max", "med", "q1", "q3", "sum",
];
const DTYPES: &[&str] = &["Byte", "Int8", "UInt16", "Int16", "UInt32", "Int32", "UInt64", "Int64", "Float32", "Float64"];

/// Value suggestions for options whose flag name alone identifies them.
pub fn choices_for(cmd_key: &str, long: &str) -> Vec<&'static str> {
    let by_cmd: Option<&[&str]> = match (cmd_key, long) {
        ("mosaic", "--resolution") => Some(&["highest", "lowest", "average", "user"]),
        ("grid", "--method") => Some(&["statistics", "linear", "nearest"]),
        ("filter outlier", "--method") => Some(&["statistical", "radius"]),
        ("filter voxeldownsize", "--mode") => Some(&["center", "first"]),
        _ => None,
    };
    if let Some(c) = by_cmd {
        return c.to_vec();
    }
    match long {
        "--compress" => COMPRESS.to_vec(),
        "--resampling" => RESAMPLING.to_vec(),
        "--ot" | "--type" => DTYPES.to_vec(),
        "--alg" => vec!["Horn", "ZevenbergenThorne", "Riley", "Wilson"],
        "--extent" => vec!["ignore", "fail", "union", "intersect"],
        "--distunits" => vec!["PIXEL", "GEO"],
        "--nlt" => vec![
            "PROMOTE_TO_MULTI", "POINT", "LINESTRING", "POLYGON", "MULTIPOINT", "MULTILINESTRING",
            "MULTIPOLYGON", "GEOMETRYCOLLECTION", "GEOMETRY", "CONVERT_TO_LINEAR",
        ],
        "--ply-encoding" => vec!["ascii", "binary_little_endian", "binary_big_endian"],
        "--ept-data-type" => vec!["laszip", "binary", "zstandard"],
        "--stat" => vec!["mean", "min", "max", "idw", "count", "stdev", "all"],
        _ => vec![],
    }
}

/// Options that take a file: show a browse button.
pub const OPEN_PICKERS: &[&str] = &["--cutline", "--like", "--mask"];

/// Options the GUI fills in itself.
pub fn skipped(spec_key: &str) -> &'static [&'static str] {
    match spec_key {
        "calc" => &["--outfile"],
        _ => &[],
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Io,
    Dem,
    Calc,
    Info,
    Crs,
    Formats,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Together {
    No,
    Optional,
    Only,
}

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub key: &'static str,
    pub title: &'static str,
    pub kind: Kind,
    pub patterns: &'static str,
    pub ext: &'static str,
    pub suffix: &'static str,
    pub together: Together,
    /// Which formats (by kind) the `--format` combo offers; empty = all writable.
    pub fmt_kinds: &'static [&'static str],
    pub defaults: &'static [(&'static str, &'static str)],
    pub subs: bool,
    /// Offers the Image / Vector / Point cloud selector.
    pub typed: bool,
    pub out_name: &'static str,
}

const fn spec(key: &'static str, title: &'static str, kind: Kind, patterns: &'static str, ext: &'static str, suffix: &'static str) -> Spec {
    Spec {
        key, title, kind, patterns, ext, suffix,
        together: Together::No, fmt_kinds: &[], defaults: &[], subs: false, typed: false, out_name: "result",
    }
}

// ---------------------------------------------------------------------------
// Convert handles three kinds of input; each shows only its own options.
// ---------------------------------------------------------------------------
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum InputType {
    #[default]
    Image,
    Vector,
    PointCloud,
    All,
}

impl InputType {
    pub const ALL: [InputType; 4] = [InputType::Image, InputType::Vector, InputType::PointCloud, InputType::All];

    /// English label (translated at display time).
    pub fn label(&self) -> &'static str {
        match self {
            InputType::Image => "Image",
            InputType::Vector => "Vector",
            InputType::PointCloud => "Point cloud",
            InputType::All => "All options",
        }
    }

    fn bit(&self) -> u8 {
        match self {
            InputType::Image => 1,
            InputType::Vector => 2,
            InputType::PointCloud => 4,
            InputType::All => 7,
        }
    }

    /// Starting values for this type: file patterns, output extension, option defaults.
    pub fn starting_point(&self) -> Option<(&'static str, &'static str, &'static [(&'static str, &'static str)])> {
        match self {
            InputType::Image => Some((RASTER_PAT, ".tif", &[("--format", "COG"), ("--compress", "DEFLATE"), ("--tile-size", "512")])),
            InputType::Vector => Some((VECTOR_PAT, ".gpkg", &[("--format", "GPKG")])),
            InputType::PointCloud => Some((CLOUD_PAT, ".copc.laz", &[])),
            InputType::All => None,
        }
    }
}

/// The options that every type's starting point may set; cleared when the type changes.
pub const TYPE_MANAGED: &[&str] = &["--format", "--compress", "--tile-size"];

/// Is this option meaningful for `ty`? terrio groups most options under "Vector options" and
/// "Point cloud options"; the shared "Options" group mixes all three, so those are listed here.
/// Anything not recognised (e.g. an option added by a newer terrio) is shown for every type.
pub fn relevant(cmd_key: &str, section: &str, long: &str, ty: InputType) -> bool {
    if cmd_key != "convert" || ty == InputType::All {
        return true;
    }
    const R: u8 = 1;
    const V: u8 = 2;
    const P: u8 = 4;
    let kinds = match section {
        "Vector options" => V,
        "Point cloud options" => P,
        "Options" => match long {
            "--format" | "--driver" | "--t-srs" => R | V | P,
            "--oo" | "--minzoom" | "--maxzoom" => R | V,
            "--layer" | "--lco" | "--dsco" => V,
            "--compress" | "--tile-size" | "--bigtiff" | "--srcwin" | "--overview" | "--co" => R,
            _ => R | V | P,
        },
        _ => R | V | P,
    };
    kinds & ty.bit() != 0
}

pub fn specs() -> Vec<Spec> {
    const R: &[&str] = &["raster"];
    vec![
        spec("info", "Info", Kind::Info, ANY_PAT, "", ""),
        Spec { defaults: &[("--format", "COG"), ("--compress", "DEFLATE"), ("--tile-size", "512")], typed: true,
               ..spec("convert", "Convert", Kind::Io, RASTER_PAT, ".tif", "_converted") },
        Spec { fmt_kinds: R, ..spec("translate", "Translate", Kind::Io, RASTER_PAT, ".tif", "_translated") },
        Spec { together: Together::Optional, fmt_kinds: R, out_name: "warped",
               ..spec("warp", "Warp", Kind::Io, RASTER_PAT, ".tif", "_warped") },
        Spec { together: Together::Only, fmt_kinds: R, out_name: "mosaic",
               ..spec("mosaic", "Mosaic", Kind::Io, RASTER_PAT, ".tif", "") },
        Spec { fmt_kinds: R, ..spec("rasterize", "Rasterize", Kind::Io, VECTOR_PAT, ".tif", "_raster") },
        spec("polygonize", "Polygonize", Kind::Io, RASTER_PAT, ".gpkg", "_polygons"),
        Spec { fmt_kinds: R, ..spec("dem", "DEM", Kind::Dem, RASTER_PAT, ".tif", "_hillshade") },
        Spec { together: Together::Optional, fmt_kinds: R, out_name: "calc",
               ..spec("calc", "Calc", Kind::Calc, RASTER_PAT, ".tif", "_calc") },
        Spec { fmt_kinds: R, ..spec("proximity", "Proximity", Kind::Io, RASTER_PAT, ".tif", "_proximity") },
        Spec { fmt_kinds: R, ..spec("sieve", "Sieve", Kind::Io, RASTER_PAT, ".tif", "_sieved") },
        Spec { fmt_kinds: R, ..spec("grid", "Grid points", Kind::Io, CLOUD_PAT, ".tif", "_grid") },
        Spec { together: Together::Optional, subs: true, out_name: "merged",
               ..spec("filter", "Point filters", Kind::Io, CLOUD_PAT, ".laz", "_filtered") },
        spec("crs", "CRS", Kind::Crs, "", "", ""),
        spec("formats", "Formats", Kind::Formats, "", "", ""),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_options_follow_the_input_type() {
        let r = |sec, long, ty| relevant("convert", sec, long, ty);
        assert!(r("Options", "--compress", InputType::Image));
        assert!(!r("Options", "--compress", InputType::Vector));
        assert!(!r("Options", "--layer", InputType::Image));
        assert!(r("Options", "--layer", InputType::Vector));
        assert!(r("Vector options", "--where", InputType::Vector));
        assert!(!r("Vector options", "--where", InputType::PointCloud));
        assert!(r("Point cloud options", "--bounds", InputType::PointCloud));
        assert!(!r("Point cloud options", "--bounds", InputType::Image));
        assert!(r("Options", "--format", InputType::PointCloud));
        assert!(r("Options", "--brand-new-flag", InputType::Vector), "unknown options stay visible");
        assert!(r("Options", "--compress", InputType::All));
        assert!(relevant("warp", "Options", "--compress", InputType::Vector), "only convert is filtered");
    }
}
