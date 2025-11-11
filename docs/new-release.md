# Release Process

This document describes how to create a new release of stui.

## Prerequisites

- Push access to the `corcoran/stui` GitHub repository
- GitHub Actions enabled in repository settings
- All changes committed and pushed to `master` branch
- All tests passing: `cargo test`

## Release Steps

### 1. Update Version Number

Edit `Cargo.toml` and update the version:

```toml
[package]
name = "stui"
version = "0.10.0"  # <-- Update this line
edition = "2021"
```

### 2. Test Build

Verify everything compiles and tests pass:

```bash
cargo build --release
cargo test
```

Expected: All tests pass, no warnings.

### 3. Commit Version Bump

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: Bump version to 0.10.0"
git push origin master
```

### 4. Create and Push Tag

Create a git tag matching the version number with `v` prefix:

```bash
git tag v0.10.0
git push origin v0.10.0
```

**Important:** The tag must match the pattern `v*.*.*` (e.g., `v0.10.0`, `v1.0.0`, `v1.2.3`)

### 5. Monitor GitHub Actions

The tag push triggers the release workflow automatically.

**Check workflow progress:**
1. Go to: `https://github.com/corcoran/stui/actions`
2. Look for the "Release" workflow with your tag name
3. Click on it to see build progress

**Expected workflow steps:**
- 5 parallel build jobs:
  - Linux x86_64 (musl - statically linked)
  - Linux ARM64 (musl - statically linked)
  - macOS Intel
  - macOS Apple Silicon
  - Windows x86_64
- 1 release job (runs after all builds complete)

**Build time:** Approximately 10-15 minutes for all platforms.

### 6. Review Draft Release

After the workflow completes:

1. Go to: `https://github.com/corcoran/stui/releases`
2. Find the draft release for your tag
3. Review the release notes and attached binaries

**Expected artifacts:**
- `stui-linux-x86_64.tar.gz` (statically linked, works on all distros)
- `stui-linux-aarch64.tar.gz` (statically linked)
- `stui-macos-intel.tar.gz`
- `stui-macos-arm64.tar.gz`
- `stui-windows-x86_64.zip`

Each archive contains:
- The compiled binary (`stui` or `stui.exe`)
- `README.md`
- `config.yaml.example`

### 7. Edit Release Notes (Optional)

Add release notes describing what's new:

```markdown
## What's New

- Feature: Brief description
- Fix: Brief description
- Improvement: Brief description

## Breaking Changes

(If any)

## Installation

(Auto-generated instructions are already included)
```

### 8. Publish Release

Click **"Publish release"** to make it public.

The release will be visible at: `https://github.com/corcoran/stui/releases/tag/v0.10.0`

## Troubleshooting

### GitHub Actions Didn't Run

**Check if Actions is enabled:**
1. Go to: `https://github.com/corcoran/stui/settings/actions`
2. Under "Actions permissions", ensure **"Allow all actions and reusable workflows"** is selected
3. Click **Save**

**Re-trigger the workflow:**
If Actions was disabled when you pushed the tag, delete and re-push it:

```bash
# Delete tag locally and remotely
git tag -d v0.10.0
git push origin :refs/tags/v0.10.0

# Re-create and push tag
git tag v0.10.0
git push origin v0.10.0
```

## Technical Details

### Build Targets

**Linux:**
- Uses `musl` for fully static binaries
- No GLIBC dependency - works on any Linux distribution
- Targets: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`

**macOS:**
- Separate builds for Intel and Apple Silicon
- Runners: `macos-13` (Intel), `macos-14` (Apple Silicon)

**Windows:**
- Standard MSVC toolchain
- Target: `x86_64-pc-windows-msvc`

### Version Information

The `--version` flag includes:
- Version number from `Cargo.toml`
- Git hash (7 characters) from `build.rs`
- Build date from `build.rs`

Example: `stui 0.9.0 (82587c7) - 2025-11-11`

### Workflow File

The release workflow is defined in: `.github/workflows/release.yml`

**Trigger:** Push of tags matching `v*.*.*`

**Workflow steps:**
1. Checkout code
2. Install Rust toolchain
3. Install platform-specific build tools (musl, cross-compilers)
4. Build release binary
5. Create archive with binary + docs
6. Upload artifacts
7. Create GitHub release with all artifacts

## Release Checklist

Before creating a release:

- [ ] All tests pass: `cargo test`
- [ ] No compiler warnings: `cargo build --release`
- [ ] Version updated in `Cargo.toml`
- [ ] Version bump committed and pushed
- [ ] CHANGELOG updated (if applicable)
- [ ] Tag created and pushed
- [ ] GitHub Actions workflow completed successfully
- [ ] All 5 platform binaries present in release
- [ ] Draft release reviewed
- [ ] Release notes added (optional)
- [ ] Release published

## Example Release

```bash
# 1. Update version
vim Cargo.toml  # Change to 0.10.0

# 2. Test
cargo build --release && cargo test

# 3. Commit
git add Cargo.toml Cargo.lock
git commit -m "chore: Bump version to 0.10.0"
git push origin master

# 4. Tag and push
git tag v0.10.0
git push origin v0.10.0

# 5. Monitor at https://github.com/corcoran/stui/actions

# 6. Review draft at https://github.com/corcoran/stui/releases

# 7. Publish release
```
