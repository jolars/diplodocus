import unittest

from experiment import HostLinks, document_urls


class HostLinkTests(unittest.TestCase):
    def test_remounts_semantic_links_and_figures_without_changing_external_urls(self):
        parser = HostLinks(
            "packages/r/reference/euler.html",
            {
                "packages/python/reference/function.html": "/docs/reference/python/euler/",
            },
        )
        parser.feed("""<a href="../../python/reference/function.html#overload-1">Python</a>
<img src="../../../assets/figure.png" alt="A &amp; B">
<a href="#examples">Examples</a><a href="https://example.org/?a=1&amp;b=2">Source</a>""")
        body = "".join(parser.parts)
        self.assertIn('href="/docs/reference/python/euler/#overload-1"', body)
        self.assertIn('src="/_diplodocus/figure.png"', body)
        self.assertIn('alt="A &amp; B"', body)
        self.assertIn('href="#examples"', body)
        self.assertIn('href="https://example.org/?a=1&amp;b=2"', body)

    def test_rejects_unmapped_local_links(self):
        with self.assertRaisesRegex(ValueError, "Unmapped local URL"):
            HostLinks("index.html", {}).feed('<a href="missing.html">Missing</a>')

    def test_keeps_wide_tables_in_a_scroll_container(self):
        parser = HostLinks("index.html", {})
        parser.feed("<table><tr><td>Value</td></tr></table>")
        self.assertEqual(
            "".join(parser.parts),
            '<div class="table-scroll"><table><tr><td>Value</td></tr></table></div>',
        )

    def test_public_python_label_controls_the_host_route(self):
        urls = document_urls(
            [
                {
                    "path": "packages/python/reference/opaque.html",
                    "title": "eunoia.euler",
                    "qualified_name": "eunoia._fit.euler",
                    "package": "python",
                }
            ]
        )
        self.assertEqual(
            urls["packages/python/reference/opaque.html"],
            "/docs/reference/python/euler/",
        )

    def test_route_collisions_fail_before_writing_pages(self):
        with self.assertRaisesRegex(ValueError, "Duplicate host route"):
            document_urls(
                [
                    {
                        "path": path,
                        "title": "eunoia.euler",
                        "qualified_name": "eunoia._fit.euler",
                        "package": "python",
                    }
                    for path in ["first.html", "second.html"]
                ]
            )


if __name__ == "__main__":
    unittest.main()
