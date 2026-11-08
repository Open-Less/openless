# Awareness follow-up

The desktop Awareness page groups cursor context, opt-in correction suggestions, and an initial opt-in expression-learning implementation. The initial implementation learns only a small set of global formatting directions, stores abstract evidence locally, and applies stable directions to future dictation prompts. It does not yet implement application-awareness controls, per-app/category profiles, a user blacklist, or the complete correction system requested by the user.

## Reference and evidence

A local Type4Me source checkout at upstream revision `f21339e214d65e9bd4d191ff5136513902e7473b` was reviewed. The installed app identifies itself as 3.2.0; equivalence to that checkout has not been established. The checkout is MIT-licensed: retain its copyright and license notice if source or substantial portions are ported. Relevant upstream files are `Sources/Type4Me/Services/IntelliSenseContext.swift`, `UserEditClassifier.swift`, `ExpressionProfileStore.swift`, `BatchCorrectionInference.swift`, and `Sources/Type4MeIntelliSenseCore/IntelliSensePrompt.swift` in https://github.com/joewongjc/type4me.

The reviewed implementation separates app/control context, cursor context, correction detection, and expression learning. It classifies observed edits and rejects sensitive content and changes to facts or negation before treating edits as style evidence. Style profiles hold abstract features and use sample, day-span, consistency and decay thresholds before emitting limited form-only directives. Its batched correction candidates use independent sessions, multiple days and directional confidence. These are observations about that source revision, not claims about an inaccessible server or later app version.

## Required implementation

- Define independently controlled, opt-in learning and capture boundaries; keep secure fields, sensitive apps, uncertain target changes and blacklist entries excluded. Describe when contextual text may be sent to the configured model.
- The initial implementation accepts only uniquely anchored formatting edits, rejects sensitive or content-changing observations, and stores bounded text-free global evidence. Opt-out stops observation and new-session application; users can clear the profile. Mixed edits and richer classification remain unsupported.
- Extend the profile to app/category scopes with explicit precedence and decay, plus a user-maintained exclusion list. Validate reset and opt-out races across all hosts, and ensure form directives cannot override facts, explicit corrections or user instructions.
- Separate correction suggestion detection from style learning; retain explicit user confirmation for learned vocabulary. Cover conflicting candidates and repeated sessions without treating one edit as a stable habit.
- Integrate the controls into the single Awareness page and exercise opt-out, clearing, app exclusion, restart and prompt behavior across supported hosts. Add deterministic classification, persistence, safety, stale-observer and native integration tests before enabling the feature.
