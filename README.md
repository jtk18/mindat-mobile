# Mindat Mobile

A Tauri 2.0 iOS app for finding mineral localities near your GPS location using the [Mindat](https://mindat.org) database.

## Features

- GPS-based locality search with interactive map
- Element symbol detection (Cu, Fe, Au, etc.) for mineral filtering
- Name-based locality search (Copper, Mine, etc.)
- Settings menu with km/mi toggle (miles default)
- Load More pagination for additional results
- State detection from GPS coordinates

## Requirements

- macOS with Xcode installed
- Rust toolchain with iOS targets
- Mindat API key (get one at [mindat.org](https://www.mindat.org/api-doc))
- Apple Developer account (for iOS builds)

## Setup

### 1. Install dependencies

```bash
# Install Rust iOS targets
rustup target add aarch64-apple-ios aarch64-apple-ios-sim

# Install JS dependencies
pnpm install
```

### 2. Configure Apple Development Team

Set your Apple Developer Team ID via environment variable:

```bash
export APPLE_DEVELOPMENT_TEAM="YOUR_TEAM_ID"
```

Or configure it in Xcode after running `cargo tauri ios init`.

### 3. Run on iOS Simulator

```bash
cargo tauri ios dev
```

### 4. Enter API Key

On first launch, enter your Mindat API key in the app.

## Known Limitations

- Mindat API ignores `page_size` parameter (returns ~10 per page)
- No server-side GPS filtering for element searches (filtered client-side)
- Search for elements like "Cu" returns all USA localities, then filters by radius
