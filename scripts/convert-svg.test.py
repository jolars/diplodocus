# /// script
# requires-python = ">=3.11"
# dependencies = ["cairosvg==2.8.2", "defusedxml==0.7.1"]
# ///
"""Run with uv run scripts/convert-svg.test.py."""

from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from defusedxml.common import EntitiesForbidden

convert = runpy.run_path(str(Path(__file__).with_name("convert-svg.py")))["convert"]


def svg(body):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" '
            f'xmlns:xlink="http://www.w3.org/1999/xlink" '
            f'width="20" height="10">{body}</svg>').encode()


class ConversionTests(unittest.TestCase):
    def test_matplotlib_features_preserve_pixels(self):
        source = b'''<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN"
          "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">''' + svg('''
          <metadata><anything xmlns="urn:metadata">palette</anything></metadata>
          <defs><style>*{stroke-linejoin: round; stroke-linecap: butt}</style>
            <path id="swatch" d="M0 0H20V10H0Z"/>
            <clipPath id="clip"><rect width="10" height="10"/></clipPath>
          </defs>
          <g clip-path="url(#clip)" style="fill: #ff0000">
            <use xlink:href="#swatch"/>
          </g>''')
        with patch("urllib.request.urlopen", side_effect=AssertionError("network access")):
            actual = convert(source)
        self.assertEqual(actual, convert(svg('<rect width="10" height="10" fill="red"/>')))
        self.assertTrue(actual.startswith(b"\x89PNG\r\n\x1a\n"))

    def test_active_content_and_secondary_resources_fail(self):
        for body in [
            '<script>alert(1)</script>', '<foreignObject/>', '<animate/>',
            '<rect onclick="alert(1)"/>',
            '<use href="https://example.invalid/x.svg#p"/>',
            '<use href="file:///etc/passwd#p"/>',
            '<use href="data:image/svg+xml,anything"/>',
            '<style>@import "https://example.invalid/style.css";</style>',
            '<style>@import "file:///etc/passwd";</style>',
            '<style>@import "data:text/css,*{fill:red}";</style>',
        ]:
            with self.subTest(body=body), self.assertRaises(ValueError):
                convert(svg(body))

    def test_entities_are_rejected(self):
        for declaration in ['<!ENTITY x "red">',
                            '<!ENTITY x SYSTEM "file:///etc/passwd">']:
            with self.subTest(declaration=declaration), self.assertRaises(EntitiesForbidden):
                convert(f'<!DOCTYPE svg [{declaration}]>'.encode() + svg('<title>&x;</title>'))

    def test_cli_reports_input_failures_and_preserves_existing_outputs(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "figure.svg"
            source.write_bytes(svg('<rect width="10" height="10"/>'))
            command = [sys.executable, str(Path(__file__).with_name("convert-svg.py")), str(source)]
            first = subprocess.run(command, capture_output=True)
            self.assertEqual(first.returncode, 0, first.stderr)
            destination = source.with_suffix(".png")
            before = destination.read_bytes()
            second = subprocess.run(command, capture_output=True)
            self.assertNotEqual(second.returncode, 0)
            self.assertIn(str(source).encode(), second.stderr)
            self.assertEqual(destination.read_bytes(), before)
            destination.unlink()
            source.write_bytes(svg('<script/>'))
            third = subprocess.run(command, capture_output=True)
            self.assertNotEqual(third.returncode, 0)
            self.assertIn(b"Unsupported static SVG element", third.stderr)
            self.assertFalse(destination.exists())


if __name__ == "__main__":
    unittest.main()
