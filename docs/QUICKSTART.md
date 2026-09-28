# Quick Start - Get Running in Minutes!

For experienced developers who want to get started immediately.

## Prerequisites
- ADB installed and configured in path on your system (I may or may not be include a autoconfigure feature later).
- Android SDK command-line tools and JDK 21 (for mobile builds)
- Phone and PC on same WiFi network

## 1. Install All Dependencies (5 min)

```bash

# Android App
./scripts/build-android.sh debug

# Desktop Client
cd desktop && pnpm install && cd ..
```

## 2. Start Everything (2 min)

Open 1 terminal:

**Terminal 1 - Desktop Client:**
```bash
cd desktop && pnpm run dev
```
**Android build**
```bash 
cd android && ./gradlew :app:assembleDebug
```

## 4. Connect (2 min)

1. Connect your phone via USB if want USB or note your phone's ip if you don't want USB.
2. Mobile: Grant permissions → Server starts automatically (Click pause icon if you want to stop it).
3. Desktop: Click "Connect".
4. Done! Video should be streaming.

## Common Issues & Quick Fixes

**Can't connect?**
```bash
# Firewall blocking port 8080? Allow it:
# Windows: Windows Defender Firewall → Allow an app
# Mac: System Preferences → Security → Firewall Options
# Linux: sudo ufw allow 8080
```

**Android build errors?**
```bash
cd android && ./gradlew clean
```

## Tips for Best Performance (for low end phones)

- Use 720p or 480p(balance of quality/performance)
- 30 FPS for most use cases
- 5GHz WiFi >> 2.4GHz WiFi
- Close unnecessary apps on phone

## Building Release Versions

**Desktop (Linux):**
```bash
./scripts/build-linux-client.sh
```

## That's It!

You now have a working mobile webcam system. Customize, improve, share!

For USB setup, see [USB_CONNECTION.md](USB_CONNECTION.md).
