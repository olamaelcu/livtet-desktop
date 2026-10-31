# 46. Refresh an expired ATProto session from settings

Date: 2026-10-31

Status: Accepted

## Context

The account panel shows when the stored access token expires. It could pass that
time and then keep showing it, with no way to act: `status` reads the stored
session and nothing else, so the panel reported an expiry that had already gone
by while the only refresh path was an incidental side effect of another call
(`ensure_fresh`, reached through profile reads).

That path also refused the case it exists for. Its expiry branch returned early
when the token had *already* expired, so a refresh only ever happened inside the
proactive window before expiry, or when no expiry was recorded at all.

## Decision

`needs_refresh` decides the policy in one place: refresh when the token has
expired, when it expires inside `REFRESH_BEFORE_EXPIRY_SECS`, or when no expiry is
recorded; leave a comfortably valid token alone, because a refresh token is
single-use and every refresh rotates it.

The manager exposes `refresh_session()`, and the `atproto_refresh_session` command
exposes it to the webview. Settings shows the button only once the expiry it is
displaying has passed, and reports what happened: a moved `expires_at` means the
authorization server issued a new token, an unchanged one means the old token was
still accepted.

## Consequences

The panel can recover a session without signing in again, and the expiry it shows
is the one actually stored.

The command reports the session as stored afterwards, so it is not a general
"force a new token": while the token is comfortably valid it returns the existing
session rather than spending the refresh token. A PDS that accepts an expired
token leaves the stored expiry unchanged, and the panel says so rather than
pretending the value moved.
An expired stored expiry is a claim about a timestamp, not about validity, and the
panel words it that way. When a refresh comes back with the same expiry the PDS
was still serving that token, so the panel drops the warning for the rest of the
session instead of sending the reader off to sign in again; the stored timestamp
itself is left alone, because nothing on this side can know the real lifetime the
server chose.
