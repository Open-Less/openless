# OpenLess security backport

This directory contains the sources from the published
`wayland-scanner 0.31.10` crate (upstream commit
`a3d7927d87799b2955bf491b51c7c2a3a82da661`). The source-package changes
backport upstream commits `ec2d93285559` and `d07c4f91f28b`:

- bump `quick-xml` from `0.39` to `0.41`;
- use the equivalent `xml10_content()` API required by the newer release.

The local patch can be removed after upstream publishes a compatible
`wayland-scanner` release containing that security fix.
