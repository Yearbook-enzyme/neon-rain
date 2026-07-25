# Upstream nixpkgs submission

Neon Rain already provides its own Nix flake. Upstream nixpkgs inclusion is a separate contribution that will allow commands such as:

```bash
nix run nixpkgs#neon-rain
```

and NixOS configuration such as:

```nix
environment.systemPackages = with pkgs; [ neon-rain ];
```

## 1. Fork nixpkgs

Open the `NixOS/nixpkgs` repository on GitHub and create a fork under `Yearbook-enzyme`.

Clone the fork and add the canonical repository as `upstream`:

```bash
mkdir -p ~/Projects
cd ~/Projects
git clone https://github.com/Yearbook-enzyme/nixpkgs.git
cd nixpkgs
git remote add upstream https://github.com/NixOS/nixpkgs.git
```

A nixpkgs checkout is large, so the initial clone can take some time.

## 2. Generate and test the contribution

From the Neon Rain repository:

```bash
scripts/prepare-nixpkgs-pr.sh ~/Projects/nixpkgs
```

The script:

- updates the fork branch from upstream `master`
- creates `neon-rain-init`
- adds the package under `pkgs/by-name/ne/neon-rain/package.nix`
- adds the maintainer entry
- computes the tagged source hash
- discovers the locked Cargo dependency hash
- builds Neon Rain
- validates the maintainer list
- formats the Nix files
- commits the result

## 3. Push and open the pull request

```bash
cd ~/Projects/nixpkgs
git push -u origin neon-rain-init
```

Open a pull request targeting `NixOS/nixpkgs:master` with the title:

```text
neon-rain: init at 0.1.0-alpha.3
```

Suggested body:

```markdown
Neon Rain is a GPU-accelerated, music-reactive Matrix rain visualizer written in Rust with wgpu.

- Built on x86_64-linux
- Tested on NixOS/KDE Wayland with an AMD Radeon RX 580
- Includes desktop integration, packaged fonts, PipeWire tools, and MPRIS support
- Upstream release: v0.1.0-alpha.3

## Things done

- [x] Built on x86_64-linux
- [x] Tested basic functionality on NixOS
- [x] Tested `nix-build -A neon-rain`
- [x] Tested `nix-build lib/tests/maintainers.nix`
```

Before submitting, review the generated diff yourself and ensure the package still launches from the result being proposed.

## 4. Runtime smoke test from the nixpkgs checkout

```bash
cd ~/Projects/nixpkgs
result/bin/neon-rain --version
result/bin/neon-rain --list-scenes
result/bin/neon-rain
```

The upstream pull request should remain limited to the package and maintainer entry. Project documentation and release changes stay in the Neon Rain repository.
