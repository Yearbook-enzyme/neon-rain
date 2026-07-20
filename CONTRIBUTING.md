# Contributing to Neon Rain

Thanks for taking an interest in Neon Rain. The project is in early alpha, so contributions are most useful when they are focused, reproducible, and easy to review.

## Good first contributions

- Reproducible bug reports
- Nix and Linux packaging improvements
- Documentation corrections
- Small renderer or performance fixes
- Additional tests
- Clearly isolated accessibility or usability improvements

Large feature changes should begin as an issue so the direction can be discussed before substantial work is done.

## Development

Enter the reproducible development environment:

```bash
nix develop
```

Run the application:

```bash
cargo run --release
```

Before opening a pull request, run:

```bash
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets
nix flake check -L
```

## Bug reports

Please include:

- Operating system and desktop/session
- GPU and graphics backend
- How Neon Rain was installed or launched
- Exact steps to reproduce the problem
- Terminal output or logs
- Whether the problem occurs without optional media enrichment
- Screenshots or recordings when they clarify a visual issue

Do not include private media files, lyrics, account tokens, or personally identifying paths unless they are essential and have been redacted.

## Pull requests

Keep each pull request focused on one coherent change. Explain the user-visible effect, testing performed, and any platform assumptions.

By contributing, you agree that your contribution will be licensed under the MIT License.
