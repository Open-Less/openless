# Visual ROI feasibility boundary

Status: implemented as a capability boundary, not a user-facing feature.

OpenLess currently has no trustworthy system gaze signal on macOS, Windows, or Linux. The runtime
therefore reports `supported: false` through `probe_visual_roi_capability` and does not expose an
enable switch.

This probe guarantees:

- no screen capture;
- no OCR;
- no image, gaze, or region data is stored;
- the feature remains off by default;
- a future implementation must name its signal source and pass an explicit privacy review before
  changing `supported` to true.

Text context is handled separately through the caret-based host-document adapters: Accessibility
on macOS, TSF on Windows, and fcitx SurroundingText on Linux.
