#  Android Webcam System
Turn your Android phone into a high-quality webcam using a fully local, serverless architecture.

Open-source (GPL-3.0)
> [!NOTE]
> ## Sponsorship Needed
>
> Android Webcam Project currently supports Android and Windows.
> I'd like to expand it to iOS and macOS, but Apple development
> requires access to macOS hardware.
>
> If you'd like to support the project, sponsorships will help
> fund the hardware needed to develop and test Apple-platform
> versions.

## What's Included

This is a **complete professional solution** with:

- 📱 **Kotlin Mobile App** (Android for now)
- 💻 **Tauri Desktop Client** (Windows/Mac/Linux)
- 🔌 **USB Connection Support** (low latency, more stable)
- 📡 **WiFi Connection Support** (wireless freedom)

## Documentation

- [Quick start](docs/QUICKSTART.md)
- [USB connection](docs/USB_CONNECTION.md)
## Components

### AWC — Desktop Client
Tauri-based app that connects to your phone and outputs virtual webcam.

### AWA — Android App
Kotlin-based mobile app that acts as the streaming server.
## Features

### Mobile App
- HD/FHD/4K resolution support (480p, 720p, 1080p, 4K)
- Front/back camera switching
- USB and WiFi connection modes
- Real-time connection status
- Low battery usage
#### *(Planned)*
- Adjustable FPS (15, 30, 60 fps) 

### Desktop Client
- Virtual webcam device (works with Zoom, Teams, Meet, OBS, etc.)
- Clean, modern UI
- Connection statistics

### Connection Options
- **USB Mode**: Lower latency, more stable, no WiFi needed
- **WiFi Mode**: Wireless freedom, works anywhere

## Prerequisites

### Required
- **PC**: Windows 10+, (*Planned for* macOS and Linux)
- **Phone**: Android 8.0+

### For Development
- **Android Studio** (for Android builds)
- **VS Code** (recommended editor)
- Or your favourite software for developement


## Using as Virtual Webcam

The desktop client creates a virtual webcam that works with:

- ✅ Zoom
- ✅ Microsoft Teams
- ✅ Google Meet
- ✅ Discord
- ✅ OBS Studio
- ✅ Skype
- ✅ Any app that uses webcams!

### Windows Setup
The virtual webcam should appear automatically in your video apps.

### Linux Setup
The desktop client does not install `adb` or `v4l2loopback`. It checks for them and shows an error if they are missing.

Load a loopback device before **Start Virtual Cam**:

```bash
sudo apt-get install v4l2loopback-dkms
sudo modprobe v4l2loopback devices=1 card_label="AWC Virtual Cam" exclusive_caps=0
```

If v4l2loopback was already loaded with `exclusive_caps=1`, reload it once so
the new setting takes effect:

```bash
sudo modprobe -r v4l2loopback
sudo modprobe v4l2loopback devices=1 card_label="AWC Virtual Cam" exclusive_caps=0
```

Then pick **AWC Virtual Cam** in Zoom, Meet, or OBS. `adb` must be on PATH for USB mode.

## Configuration

### Change Resolution
Edit in mobile app settings or in code:
- 480p (640x480) - Low bandwidth
- 720p (1280x720) - **Recommended for older phones**
- 1080p (1920x1080) - High quality
- 4K (3840x2160) - Maximum quality


## Building for Production

### Android client

From the repository root:

```bash
./scripts/build-android.sh debug
./scripts/build-android.sh release
```

The APK is written to `android/app/build/outputs/apk/`.

### Desktop client (Linux)

From the repository root:

```bash
./scripts/build-linux-client.sh
```

That command builds `.deb` and AppImage. Pass `appimage` or `deb` to build one bundle.

```bash
./scripts/build-linux-client.sh appimage
```

Output:

- `desktop/src-tauri/target/release/bundle/deb/`
- `desktop/src-tauri/target/release/bundle/appimage/`

The script uses Docker when the host is missing GTK, WebKit, v4l, or FFmpeg 8 headers. Host Node, pnpm, and Rust stay required.

When those system packages are already installed:

```bash
cd desktop
pnpm install
pnpm run build:linux
```

## Troubleshooting

### Connection Issues

**"Can't connect -"**
- Check firewall (allow port 8080 and 8554 - default ports)
- Verify same WiFi network (for WiFi mode)
- Check USB debugging (for USB mode)


### Video Quality Issues

- Lower resolution to 720p
- Use USB instead of WiFi
- Close other apps using camera
- Use 5GHz WiFi if available

## Advantages Over Other APPs

| Feature | AWA | Other APPs  |
|---------|--------------|----------|
| Price | **Free** | Pay for HD and high resolutions |
| Resolution | Up to 4K | 720p (free), 1080p (paid) |
| Open Source (Customize as you want) | ✅ Yes | ❌ No |
| USB Support | ✅ Yes | ✅ Yes |
| WiFi Support | ✅ Yes | ✅ Yes |
| Customizable | ✅ Full control | ❌ No |
| Privacy | ✅ Self-hosted | ⚠️ |
| No Ads | ✅ Yes | ❌ Has ads |

## Contributing

This is an open-source project! Contributions welcome:

- 🐛 Report bugs
- 💡 Suggest features
- 🔧 Submit pull requests
- 📖 Improve documentation
- ⭐ Star the repository

## License

This project is licensed under the **GNU General Public License v3.0 (GPL-3.0)**.

© 2026 Soubhagyajit Borah

### What this means:

* ✅ You can use, modify, and distribute this software
* ✅ You can use it commercially
* ⚠️ You must disclose source code if you distribute it
* ⚠️ Any derivative work must also be licensed under GPL-3.0

See the [LICENSE](LICENSE) file for full details.


## Acknowledgments

- Virtual camera powered by [Softcam](https://github.com/tshino/softcam) © tshino (MIT License)
- Built with Kotlin, Electron.
- Inspired by DroidCam and similar tools.
- Made with ❤️ for the open-source community.

## Roadmap

- [ ] Audio streaming support
- [ ] Recording functionality
- [ ] Mobile app on app stores

## Support

Having issues? Found a bug?

1. Create a new issue with details
2. Join our community discussions

> [!TIP]
> **Enjoying Android Webcam Project?** ⭐
>
> If this project helped you, please consider leaving a **GitHub Star** and sharing your feedback. Your support helps the project grow and motivates future development!

---
> [!NOTE]
>*** *Virtual Webcam Device is back in v1.0.6. Enjoy AWP - Android Webcam Project!*
>*Virtual webcam device was not included in client version v1.0.5 as I was investing an issue. I released it in the next version with Virtual camera included. If you need the Virtual camera, please install client version v1.0.6*

**Happy streaming! Made with ❤️ and ☕ by developers, for developers.**

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/R5O327R68W)
