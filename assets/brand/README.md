# Diplodocus artwork

`diplodocus-logo.png` is the package logo, used in the README and documentation
navbar. It shows a diplodocus writing in an open book and has a transparent
background. This is the selected v5 design, generated with the built-in image
tool without a reference image using the [saved prompt](diplodocus-logo.prompt.txt).

`favicon.png` is a 64-pixel version of the logo. `favicon.ico` contains 16, 32,
and 48 pixel versions, and `apple-touch-icon.png` is 180 pixels with a near-white
background. Regenerate these icons from `diplodocus-logo.png` with ImageMagick:

```sh
magick diplodocus-logo.png -resize 64x64 favicon.png
magick diplodocus-logo.png -define icon:auto-resize=48,32,16 favicon.ico
magick diplodocus-logo.png -resize 180x180 -background '#fafbf9' -alpha remove -alpha off apple-touch-icon.png
```

The earlier `diplodocus-logo.svg` and `favicon.svg` remain as editable artwork
for the previous design. Other drafts live in the
[logo concepts](../../docs/design/logo-concepts/README.md) directory.
