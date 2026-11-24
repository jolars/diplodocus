# C++ API extraction

Diplodocus uses Doxygen to document C++ namespaces, types, functions, and other
declarations. Put `doxygen` on `PATH` before running `diplodocus check` or
building the site. If your project generates headers or a Doxyfile, run its
configuration step first. Diplodocus reads the prepared files without compiling
the library.

## Choose a Doxyfile

You can reuse a Doxyfile from your project, including a generated one such as
`build/Doxyfile.docs`. To document only public headers, create a smaller
`Doxyfile` in the package root. Adjust `INPUT` to point to your headers.

```text
PROJECT_NAME = Demo
PROJECT_NUMBER = 0.1.0
INPUT = include
RECURSIVE = YES
EXTRACT_PRIVATE = NO
EXTRACT_STATIC = NO
WARN_AS_ERROR = YES
```

Doxygen runs from the package root, so it resolves relative input paths from
there. Diplodocus requests temporary XML output automatically. Doxygen retains
its preprocessing, input filters, and warning settings. With
`WARN_AS_ERROR = YES`, Doxygen warnings can stop the build.

## Declare the package

In `diplodocus.yaml`, use `ecosystem: cpp` and a target with
`extractor: doxygen`. This example uses a repository named `source` and a
package at its root.

```yaml
project:
  name: Demo documentation
repositories:
  - id: source
    path: .
packages:
  - id: demo
    name: Demo
    slug: cpp
    ecosystem: cpp
    repository: source
    path: .
    metadata-path: Doxyfile
    targets:
      - id: api
        extractor: doxygen
        path: Doxyfile
        role: public-api
```

`metadata-path` selects the Doxyfile that supplies project metadata, such as the
name and version. The target's `path` selects the Doxyfile for extraction. Both
paths are relative to the package root and can point to the same file. Run
`diplodocus check` to validate the configuration and run Doxygen, then
`diplodocus serve` to preview the reference.

## What appears in the reference

Namespaces, types, and free function families get reference pages. Members and
individual overloads have their own anchors. Use qualified names such as
`demo::Widget::resize` to refer to declarations. Doxygen code blocks display
source. Publish standalone guides and figures through
[content collections](../configuration.md#add-pages).

If you extract a [portable snapshot](../snapshots.md), you can generate the site
from it on a machine without Doxygen or the C++ checkout. See the
[C++ extraction contract](https://github.com/jolars/diplodocus/blob/main/docs/ir/cpp-extraction.md)
for details on visibility, supported declarations, and which files Diplodocus tracks.
