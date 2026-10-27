# M4 — Browser and desktop matrix

Only rows marked **verified** were driven for real: launch, navigate to a loopback page,
evaluate JS, screenshot (`qualyx browsers --smoke`), plus the login flows in
`fixtures/web/flows` (pass, and fail at the exact step with a screenshot).

## Browsers

| Browser | Family | Protocol | Version | OS | Status | Evidence |
|---|---|---|---|---|---|---|
| Chrome | Chromium | CDP | 156.0.8078.47 | Windows 11 | **verified** | smoke + both login flows; console `login attempt demo ok`; `HTTP 404 /missing-endpoint.json` captured |
| Edge | Chromium | CDP | 156.0.4314.42 | Windows 11 | **verified** | same as Chrome |
| Chrome / Edge / Firefox | — | — | runner image | Windows, Linux, macOS (CI) | see CI | `real_browsers` test, `QUALYX_TEST_BROWSERS=1` on every OS |
| Safari | WebKit | WebDriver (safaridriver) | runner image | macOS (CI) | see CI | needs `safaridriver --enable` once |
| Chromium, Brave, Opera, Vivaldi | Chromium | CDP | — | — | **unknown** | detected by path, never run on a test machine |
| Zen | Firefox | WebDriver BiDi | — | — | **unknown** | detected by path, never run on a test machine |

Notes:

- Playwright's bundled Firefox and WebKit are **not** branded Firefox/Safari. Qualyx drives the
  installed branded browsers directly.
- Safari exposes no console or network events through WebDriver, so those fields stay empty
  for it by design.
- On Ubuntu 24.04, AppArmor blocks the user namespaces Chrome's sandbox needs. CI lifts that
  with `sysctl` on the runner. Qualyx never adds `--no-sandbox`.

## Playwright projects

`playwright.config.*` → `npx playwright test --reporter=line,junit`. Verified on Windows 11 with
`@playwright/test` 1.64.1 (Chromium): the passing fixture gives 2 passed + 1 skipped, and the
failing one fails on exactly the seeded test with `Expected: "Welcome, demo"`. CI repeats this
on Linux.

## Desktop (Computer Use, read-only)

| OS | Session | Accessibility tree | Screenshot | Input | Evidence |
|---|---|---|---|---|---|
| Windows 11 | interactive | **verified** (UI Automation, top-level windows) | **verified** (PNG) | not implemented | `QUALYX_TEST_DESKTOP=1 cargo test -p qualyx-desktop` |
| macOS | Aqua | needs Accessibility permission | `screencapture` (Screen Recording permission) | not implemented | not run |
| Linux X11 | X11 | `wmctrl` | ImageMagick `import` | not implemented | not run |
| Linux Wayland | Wayland | unavailable | unavailable | not implemented | portals need user consent; Qualyx doesn't bypass them |

Input injection is deliberately not implemented. It needs per-OS consent flows and an approval
gate, and claiming it without them would be unsafe.
