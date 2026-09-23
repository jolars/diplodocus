//! The `svg-mvp-v1` allowlist rejects candidates rather than rewriting them.

use svgtypes::{
    Length, LengthListParser, Number, NumberListParser, PathParser, PathSegment,
    TransformListParser, TransformListToken,
};

use super::AssetError;

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

pub(super) fn validate(bytes: &[u8]) -> Result<(), AssetError> {
    validate_with_accessibility(bytes, false)
}

pub(super) fn validate_authored(bytes: &[u8]) -> Result<(), AssetError> {
    validate_with_accessibility(bytes, true)
}

fn validate_with_accessibility(bytes: &[u8], accessibility: bool) -> Result<(), AssetError> {
    let text = std::str::from_utf8(bytes).map_err(|_| AssetError::UnsafeSvg)?;
    if text.contains("<!DOCTYPE") || text.contains("<!ENTITY") {
        return Err(AssetError::UnsafeSvg);
    }
    let document = roxmltree::Document::parse(text).map_err(|_| AssetError::UnsafeSvg)?;
    let root = document.root_element();
    if root.tag_name().name() != "svg"
        || root
            .tag_name()
            .namespace()
            .is_some_and(|ns| ns != SVG_NAMESPACE)
    {
        return Err(AssetError::UnsafeSvg);
    }
    for node in document.descendants() {
        if node.is_pi() {
            return Err(AssetError::UnsafeSvg);
        }
        if !node.is_element() {
            continue;
        }
        if node.tag_name().namespace() != root.tag_name().namespace()
            || node
                .namespaces()
                .any(|ns| ns.name().is_some() || ns.uri() != SVG_NAMESPACE)
            || !matches!(
                node.tag_name().name(),
                "svg"
                    | "g"
                    | "path"
                    | "rect"
                    | "circle"
                    | "ellipse"
                    | "line"
                    | "polyline"
                    | "polygon"
                    | "text"
                    | "tspan"
                    | "title"
                    | "desc"
            )
        {
            return Err(AssetError::UnsafeSvg);
        }
        for attribute in node.attributes() {
            let accessible = accessibility
                && !attribute.value().trim().is_empty()
                && matches!(
                    attribute.name(),
                    "id" | "aria-label" | "aria-labelledby" | "aria-describedby"
                );
            if attribute.namespace().is_some()
                || !(accessible || value(attribute.name(), attribute.value().trim()))
            {
                return Err(AssetError::UnsafeSvg);
            }
        }
    }
    Ok(())
}

fn value(name: &str, value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    match name {
        "role" => value == "img",
        "x" | "y" | "dx" | "dy" => LengthListParser::from(value)
            .collect::<Result<Vec<_>, _>>()
            .is_ok_and(|values| !values.is_empty() && values.iter().all(|v| v.number.is_finite())),
        "x1" | "y1" | "x2" | "y2" | "cx" | "cy" => length(value, false),
        "width" | "height" | "r" | "rx" | "ry" | "stroke-width" => length(value, true),
        "viewBox" => numbers(value).is_some_and(|v| v.len() == 4 && v[2] >= 0.0 && v[3] >= 0.0),
        "points" => numbers(value).is_some_and(|v| !v.is_empty() && v.len() % 2 == 0),
        "d" => PathParser::from(value)
            .collect::<Result<Vec<_>, _>>()
            .is_ok_and(|segments| !segments.is_empty() && segments.into_iter().all(finite_path)),
        "transform" => TransformListParser::from(value)
            .collect::<Result<Vec<_>, _>>()
            .is_ok_and(|tokens| !tokens.is_empty() && tokens.into_iter().all(finite_transform)),
        "fill" | "stroke" => value == "none" || value.parse::<svgtypes::Color>().is_ok(),
        "fill-opacity" | "stroke-opacity" | "opacity" => {
            number(value).is_some_and(|v| (0.0..=1.0).contains(&v))
        }
        "stroke-miterlimit" => number(value).is_some_and(|v| v >= 1.0),
        "fill-rule" => matches!(value, "nonzero" | "evenodd"),
        "stroke-linecap" => matches!(value, "butt" | "round" | "square"),
        "stroke-linejoin" => matches!(value, "miter" | "round" | "bevel"),
        "font-family" => {
            !value.contains(['\\', ';', '{', '}', '(', ')'])
                && svgtypes::parse_font_families(value).is_ok_and(|v| !v.is_empty())
        }
        "font-size" => {
            length(value, true)
                || matches!(
                    value,
                    "xx-small"
                        | "x-small"
                        | "small"
                        | "medium"
                        | "large"
                        | "x-large"
                        | "xx-large"
                        | "larger"
                        | "smaller"
                )
        }
        "font-style" => matches!(value, "normal" | "italic" | "oblique"),
        "font-weight" => matches!(
            value,
            "normal"
                | "bold"
                | "bolder"
                | "lighter"
                | "100"
                | "200"
                | "300"
                | "400"
                | "500"
                | "600"
                | "700"
                | "800"
                | "900"
        ),
        "text-anchor" => matches!(value, "start" | "middle" | "end"),
        _ => false,
    }
}

fn length(text: &str, nonnegative: bool) -> bool {
    text.parse::<Length>()
        .is_ok_and(|v| v.number.is_finite() && (!nonnegative || v.number >= 0.0))
}

fn number(text: &str) -> Option<f64> {
    text.parse::<Number>()
        .ok()
        .map(|v| v.0)
        .filter(|v| v.is_finite())
}

fn numbers(text: &str) -> Option<Vec<f64>> {
    NumberListParser::from(text)
        .collect::<Result<Vec<_>, _>>()
        .ok()
        .filter(|v| v.iter().all(|v| v.is_finite()))
}

fn finite_path(segment: PathSegment) -> bool {
    match segment {
        PathSegment::MoveTo { x, y, .. }
        | PathSegment::LineTo { x, y, .. }
        | PathSegment::SmoothQuadratic { x, y, .. } => [x, y].iter().all(|v| v.is_finite()),
        PathSegment::HorizontalLineTo { x, .. } => x.is_finite(),
        PathSegment::VerticalLineTo { y, .. } => y.is_finite(),
        PathSegment::CurveTo {
            x1,
            y1,
            x2,
            y2,
            x,
            y,
            ..
        } => [x1, y1, x2, y2, x, y].iter().all(|v| v.is_finite()),
        PathSegment::SmoothCurveTo { x2, y2, x, y, .. } => {
            [x2, y2, x, y].iter().all(|v| v.is_finite())
        }
        PathSegment::Quadratic { x1, y1, x, y, .. } => [x1, y1, x, y].iter().all(|v| v.is_finite()),
        PathSegment::EllipticalArc {
            rx,
            ry,
            x_axis_rotation,
            x,
            y,
            ..
        } => {
            rx >= 0.0
                && ry >= 0.0
                && [rx, ry, x_axis_rotation, x, y]
                    .iter()
                    .all(|v| v.is_finite())
        }
        PathSegment::ClosePath { .. } => true,
    }
}

fn finite_transform(token: TransformListToken) -> bool {
    match token {
        TransformListToken::Matrix { a, b, c, d, e, f } => {
            [a, b, c, d, e, f].iter().all(|v| v.is_finite())
        }
        TransformListToken::Translate { tx, ty } => [tx, ty].iter().all(|v| v.is_finite()),
        TransformListToken::Scale { sx, sy } => [sx, sy].iter().all(|v| v.is_finite()),
        TransformListToken::Rotate { angle }
        | TransformListToken::SkewX { angle }
        | TransformListToken::SkewY { angle } => angle.is_finite(),
    }
}
