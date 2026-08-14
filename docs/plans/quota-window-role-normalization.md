# Quota Window Role Normalization Plan

## Goal

Keep account-pool quota display and automatic auth-profile selection correct when
ChatGPT returns either the legacy short-plus-weekly window pair or a single
weekly window in `primary_window`.

## Current state

The upstream `wham/usage` response for the affected Pro account is healthy and
contains a 10,080-minute window with 52 percent used, but it places that window
in `primary_window` and returns `secondary_window: null`.

The broker currently assigns meaning by response position:

- `primary` is treated as the short window;
- `secondary` is treated as the weekly window.

That assumption makes the admin top bar and account card report unknown weekly
quota even though the upstream response contains it. Auth-profile ranking also
falls back to an unweighted primary percentage instead of the weekly refresh
score.

## Proposed changes

1. Add an end-to-end regression that carries the live single-window response
   shape from the ChatGPT usage adapter through the user-facing account-pool
   formatter and auth-profile evaluator.
2. Introduce one shared quota-window role resolver that recognizes a weekly
   window from a valid `windowDurationMins` within five percent of 10,080
   minutes before considering the legacy primary/secondary position:
   - a unique duration-matched weekly window wins regardless of whether the
     upstream response places it in `primary` or `secondary`;
   - the other window, when present, is the short/other window;
   - when neither or both windows match weekly, retain the legacy
     `primary=short` and `secondary=weekly` fallback;
   - a single primary daily, monthly, or otherwise non-weekly window is not
     mislabeled or scored as weekly.
3. Remove direct weekly reads from `secondary` in quota formatting, account
   cards, top-bar items, and auth-profile scoring. Route those consumers through
   the shared resolver instead.
4. Preserve exhaustion checks for every raw upstream window so a depleted short
   or weekly limit still makes a profile unavailable.

## If we do not change it

The admin UI will continue to show `账号池额度未知`, `7d 剩余 --`, score `0`,
and an unknown reset time for valid single-weekly-window accounts. With multiple
accounts, automatic profile selection may also rank those accounts using the
wrong score.

## Expected result

The current single-weekly-window response displays the real remaining
percentage, weighted score, and reset time. Legacy 5-hour plus 7-day responses
keep their existing display and selection behavior, while generalized
non-weekly windows are not relabeled as weekly. The external admin and upstream
API contracts do not change.

## Acceptance criteria

- A live-shaped `primary=10,080 minutes`, `secondary=null` response renders 48
  percent remaining and a known reset time instead of unknown quota.
- The same response receives a weekly refresh-weighted selection score.
- Legacy `primary=300 minutes`, `secondary=10,080 minutes` behavior remains
  covered and unchanged.
- Reversed duration-bearing window positions resolve by duration, not position.
- A single monthly primary window does not become the weekly quota.
- Exhausting either raw upstream window still marks the profile unavailable.
- Targeted unit and end-to-end tests, lint, type checking/build, and a manual
  local admin-page smoke test pass.
- The change is delivered as a draft pull request and is not deployed
  automatically.
