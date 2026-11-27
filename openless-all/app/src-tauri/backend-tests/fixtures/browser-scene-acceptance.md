# P5 controlled browser acceptance

This fixture contains synthetic data only. It does not access the network, collect
entered values, submit a form or print page content. Run these steps against the
exact integrated Dev SHA after P4's native target contract is connected; the
independent P5 modules alone do not establish feature acceptance.

1. Serve this directory on loopback with `python3 -B -m http.server 8765 --bind 127.0.0.1`.
   Open `http://127.0.0.1:8765/browser-scene.html` in one supported browser.
   Grant Accessibility through macOS settings and enable this invocation's app
   and browser metadata permission. Do not grant unrelated history/context.
2. Keep the fixture's **Ask anything** textarea focused and invoke OpenLess.
   Verify the outgoing context is attributed to the original browser and is
   `webAskAnything`; only the original page location and authorized original
   selection can appear. `UNRELATED_BACKGROUND_MARKER` must not appear.
3. Focus the page's fake address field. Verify `webInput`, including when its
   ancestor is a web toolbar. It must never become `addressBar`.
4. Focus the real native address bar. Chrome/Edge English or Simplified Chinese
   can qualify only with the verified native toolbar ancestry, text-field role
   and exact accessibility name. Other browser/localization structures remain
   `browserChromeInput` or unavailable until their evidence is recorded.
   This stage performs no address/search insertion or Return action.
5. Invoke on the protected field. Verify no browser reference or field value
   reaches Ask/Agent. Repeat after disabling Accessibility; ordinary input must
   remain available.
6. Bind on the textarea, then switch tabs before model consumption. Verify the
   reference is discarded; switching back must not revive it. Repeat with
   another window, another input, another application, and a same-page URL
   query/fragment change. The bound AXWebArea and full URL fingerprint must
   change/reject even when the sanitized location stays identical.
7. Revoke the browser grant while a read is pending. Verify no data is emitted;
   regranting must require a fresh original invocation. Repeat with app metadata,
   selected-text permission and a new Ask turn.
8. In Cursor/Orca, verify canonical OS identity produces application purpose and
   coding-Agent output policy while preserving the chosen style and dictated
   scope. An unknown process named Cursor/Orca must keep default behavior.
9. Repeat the webpage, protected field, revocation and tab/focus cases in Safari,
   Google Chrome, Microsoft Edge, ego lite, Dia and the ChatGPT desktop embedded
   browser. Record app version, actual bundle id, exposed AX roles, output fields
   and exact Dev SHA. Mark unavailable structures unsupported; do not infer
   support from the app name, source code or this checklist.

Keep assertions/field names and pass/fail metadata as evidence. Do not copy user
pages, native handles, control tokens, raw URLs, field values or page text to logs.
Stop the loopback server after the acceptance session.
