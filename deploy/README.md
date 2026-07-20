# Neon Rain Linux deployment

1. Build with `cargo build --release`.
2. Run `./deploy/doctor.sh`.
3. Install for the current user with `./deploy/install-user.sh`.

Core runtime dependency: `pw-record` from PipeWire.

Optional enrichment:

- `playerctl` supplies generic MPRIS metadata and playback position.
- Neon Rain helper commands can supply moodbars, lyrics, or custom profiles.
- Without any optional integration, rolling live analysis still drives the conductor.

The application installs under XDG user directories and keeps learned numeric timelines
under the user cache directory. Removing the cache does not prevent startup.
