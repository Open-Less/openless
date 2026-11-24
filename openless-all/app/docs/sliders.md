# Slider controls

Import from `src/ui/controls` (or `src/ui/controls/Slider`). These React compositions share one Base UI Slider primitive, using the existing lockfile's `@base-ui/react` **1.9.0**, without additional dependencies.

| Origin UI reference | Export               | Composition                                                                   |
| ------------------- | -------------------- | ----------------------------------------------------------------------------- |
| 01                  | `SimpleSlider`       | Round outlined thumb                                                          |
| 03                  | `SquareSlider`       | Square outlined thumb                                                         |
| 04                  | `SolidSlider`        | Primary filled thumb, indicator at 70% opacity; production horizontal default |
| 05                  | `TinySlider`         | Narrow thumb with surface border                                              |
| 06                  | `ReferenceSlider`    | Three reference labels; customize `marks` for units                           |
| 07                  | `TickSlider`         | Ticks and alternate labels; customize `marks`                                 |
| 10                  | `TooltipSlider`      | Endpoint labels and tooltip on focus, hover or drag                           |
| 11                  | `RangeSlider`        | Multiple thumbs with collision prevention                                     |
| 20                  | `StepperSlider`      | Decrease/increase buttons and a scalar slider                                 |
| 22                  | `VerticalSlider`     | Native vertical interaction, explicit 200px default height                    |
| 265                 | `DistributionSlider` | Optional caller-supplied histogram counts and two editable bounds             |

## API

Every slider requires `aria-label`. For ranges, pass distinct `thumbLabels` (otherwise the name is suffixed with the thumb index). Scalar values are numbers; range values are readonly arrays. Use `value` with `onValueChange` for controlled usage, or `defaultValue` for uncontrolled usage. Controlled compositions always render the latest external `value`.

`onValueChange(value)` previews edits. `onValueCommitted(value)` handles completed changes using Base UI's commit timing: pointer/touch release and native keyboard/input changes. Do not add pointer-up, key-up or blur saves around the slider, which can save twice. A stepper button or accepted distribution input emits one change followed by one commit; invalid, unchanged or canceled input emits neither. Input edits commit on Enter or blur, round to `step` from `min`, and clamp to the domain and the other bound. Escape cancels the edit. External values reset any pending text draft.

`min`, `max`, `step`, `largeStep`, `disabled`, `name`, `form`, `format`, `locale`, `minStepsBetweenValues`, `thumbCollisionBehavior` and `thumbAlignment` pass through to Base UI. Keep `max > min` and `step > 0`; provide sorted range values aligned with the step. Use exactly two values for `DistributionSlider`. Decimal and negative domains are supported.

`direction="rtl"` supplies both DOM direction and Base UI's `DirectionProvider`; the existing provider can also supply direction. `orientation="vertical"` is available on the primitive; `VerticalSlider` sets it automatically. Track gestures preserve scrolling on the perpendicular axis. Native range inputs retain keyboard and touch assistive technology behavior. `marks` and `endpointLabels` are visual references; accessible names and native values are provided by the thumbs. Reference/tick/endpoint decorations are intended for horizontal use.

Use `variant`, `tooltip`, `marks`, `ticks`, `endpointLabels`, `className` and `style` on the shared `Slider` for custom compositions. `DistributionSlider.bins` contains equal-width counts across `[min, max]`; omit it to show only the range and inputs. No example distribution is inserted into production. `inputLabels`, `decreaseLabel` and `increaseLabel` let the caller localize composition controls.

Colors follow `--ol-primary-solid-bg`, `--ol-border`, `--ol-surface` and ink tokens. Override `--slider-primary`, `--slider-track`, `--slider-surface` or `--slider-thumb-size` on the root for a local appearance. Focus rings, disabled appearance and forced-color handling are included.

## References and acceptance

Visual references: the official `radix-ng/origin-ui` repository, `apps/origin-ui/src/registry/default/components/sliders/slider-{01,03,04,05,06,07,10,11,20,22,265}.ts`, corresponding to the Origin UI NG slider gallery. Runtime API and event behavior were checked against the official `mui/base-ui` **v1.9.0** `SliderRoot.tsx`, `SliderThumb.tsx` and `DirectionProvider.tsx` sources. This is a React implementation using Base UI, with no Angular code or dependency.

`e2e/sliders.spec.ts` exercises the isolated real React controls, pointer preview/commit ordering, keyboard and decimal domains, RTL, vertical and dual ranges, stepper/input bounds, cancellation, external state, native Chromium touch, and production DSP drafts. It writes `acceptance-slider-light.png` and `acceptance-slider-dark.png` through `test.info().outputPath` for each browser's CI artifacts. `e2e/android-onboarding.spec.ts` additionally exercises the production size slider and shared preference write/rollback queue.
