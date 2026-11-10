# /// script
# requires-python = ">=3.11"
# dependencies = ["cairosvg==2.8.2", "defusedxml==0.7.1"]
# ///
"""Convert static SVG figures to PNG without fetching secondary resources."""

import argparse
from pathlib import Path
import sys

from defusedxml import ElementTree
from cairosvg.surface import PNGSurface

SVG = "{http://www.w3.org/2000/svg}"
STATIC_ELEMENTS = {
    "svg", "g", "defs", "style", "path", "rect", "circle", "ellipse", "line",
    "polyline", "polygon", "text", "tspan", "title", "desc", "use", "clipPath",
}


def deny_resource(url, resource_type):
    # In-document fragment references never pass through this fetcher.
    raise ValueError(f"External or embedded resource loading is disabled ({resource_type})")


def convert(source: bytes, scale: float = 2) -> bytes:
    # DefusedXML ignores the unused Matplotlib DTD without loading it, and rejects
    # entity declarations and external entities. Never enable CairoSVG's unsafe mode.
    root = ElementTree.fromstring(source, forbid_entities=True, forbid_external=True)
    if root.tag != SVG + "svg":
        raise ValueError("Expected an SVG document")
    for parent in root.iter():
        for child in list(parent):
            if child.tag == SVG + "metadata":
                parent.remove(child)
    for node in root.iter():
        if not node.tag.startswith(SVG) or node.tag[len(SVG):] not in STATIC_ELEMENTS:
            raise ValueError(f"Unsupported static SVG element: {node.tag}")
        for name, value in node.attrib.items():
            local_name = name.rsplit("}", 1)[-1].lower()
            if local_name.startswith("on") or local_name == "base":
                raise ValueError(f"Unsupported SVG attribute: {local_name}")
            if local_name == "href" and not value.startswith("#"):
                raise ValueError("Only in-document SVG references are allowed")
    # Serializing drops XML processing instructions and the DTD. Styles and
    # clipping paths remain intact, and every renderer fetch must fail closed.
    return PNGSurface.convert(
        bytestring=ElementTree.tostring(root), scale=scale, unsafe=False,
        url_fetcher=deny_resource,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sources", nargs="+", type=Path)
    parser.add_argument("--scale", type=float, default=2,
                        help="Pixel density multiplier (default: 2)")
    args = parser.parse_args()
    if not 0 < args.scale <= 8:
        parser.error("--scale must be greater than zero and at most 8")
    failed = False
    for source in args.sources:
        destination = source.with_suffix(".png")
        try:
            if source.suffix.lower() != ".svg":
                raise ValueError("Input must have an .svg extension")
            png = convert(source.read_bytes(), args.scale)
            # Refuse to overwrite a maintained figure or publish a partial render.
            with destination.open("xb") as output:
                output.write(png)
            print(f"{source} -> {destination}")
        except Exception as error:
            failed = True
            print(f"{source}: {error}", file=sys.stderr)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
