# Bundled Wayland Protocol XML

[English](README.md) | [简体中文](README.zh.md)

`ext-data-control-v1.xml` is **copied byte for byte** from
`staging/ext-data-control/ext-data-control-v1.xml` in wayland-protocols **1.49**
(MIT; the copyright header is kept in the file). md5 `dbce6a71086f0a2eb675db5b84d9fd43`.

Why bundle it instead of using the system copy:

- The plugin has to inspect the PRIMARY selection on the compositor itself (see
  `../primary_selection.h` for why), which requires the client glue code for
  `ext_data_control_manager_v1`;
- The glue is generated from the XML at build time by `wayland-scanner`. Depending on
  the system `/usr/share/wayland-protocols/` would mean **the build machine must have
  wayland-protocols installed**, and the packaging path (`app/scripts/package-linux-egui.sh`
  takes `build/libopenless.so` directly) does not guarantee that;
- With it bundled, the build only needs `wayland-scanner` plus the `wayland-client`
  headers/library (CMake reports the missing pieces explicitly).

Upgrading: replace it with the same-named file from a newer wayland-protocols, update the
version number and md5 above, then run `../build.sh` and the two contract test cases.
