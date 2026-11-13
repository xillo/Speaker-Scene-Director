# Speaker Scene Director

A standalone desktop application written in Rust that automatically switches OBS Studio scenes based on who is speaking in TeamSpeak 6.

## Features

- TeamSpeak 6 Remote Apps (`talkStatusChanged`) integration
- OBS Studio WebSocket 5 (`SetCurrentProgramScene`)
- Automatic scene selection by TeamSpeak nickname or unique identifier
- Same-channel filtering, speaking overlap protection, brief-pause tolerance
- Reconnection after TeamSpeak or OBS disconnects
- OBS scene validation and live event log
- Participant editor, timing settings and OBS connection configuration
- VDO.Ninja publishing and viewing link generator for OBS Virtual Camera feeds; saved participant IDs are stored in `vdo_links.json`
- No telemetry, cloud service, or built-in stream relay

## Requirements

**To run:** Windows 10/11 x64, TeamSpeak 6 with Remote Apps enabled (typically port 5899), OBS Studio 28+ with WebSocket enabled (typically port 4455). To receive friends' video feeds, they can publish their OBS Virtual Camera through [VDO.Ninja](https://vdo.ninja/), and you can add the viewer links as OBS Browser Sources.

**To compile:** Rust stable with Cargo, installed through [rustup](https://rustup.rs/). GitHub Actions builds the public Windows release automatically.

## Build on Windows

Double-click **`BUILD.bat`** for the public edition, or **`BUILD_PERSONAL.bat`** to compile both the public and the personal editions; or use PowerShell:

```powershell
.\scripts\build.ps1                        # public, clean configuration
.\scripts\build.ps1 -Personal             # also import your existing config.json
.\scripts\build.ps1 -Personal -ConfigPath "W:\path\to\config.json"
```

The outputs are:

- `dist/SpeakerSceneDirector-Public.zip`: **safe for GitHub**, includes the `.exe`, documentation, and `config.example.json` with generic demo settings.
- `dist/SpeakerSceneDirector-Personal-PRIVATE.zip`: **do not publish**, includes the same `.exe` and an exact copy of your existing `config.json`, plus your saved TeamSpeak token and saved VDO.Ninja links if they exist next to the config.

`BUILD.bat` detects the previous Python install in common `W:\...` locations. If it cannot find the file, provide its exact path via `-ConfigPath`. No settings are hardcoded into the binary; the executable picks up `config.json` beside it. The personal ZIP is ignored by Git.

**Important:** The app uses a new TeamSpeak Remote Apps application identifier. On first launch you may need to approve the application once, even if an earlier Python client was already authorized. Your scene associations and delays will not need to be re-entered.

## Setup

1. Extract `SpeakerSceneDirector-Public.zip` (or your private ZIP) into a writable folder.
2. Open OBS Studio. Enable **Tools > WebSocket Server Settings**. Default port: `4455`. Keep authentication enabled for production use; enter the password in the app's Settings tab.
3. Open TeamSpeak 6, connect to your server, and enable **Remote Apps** (default port `5899`).
4. Add one OBS scene per participant. If using friends' virtual camera feeds, create each OBS Browser Source using its VDO.Ninja viewer link.
5. Launch `SpeakerSceneDirector.exe`, open **Settings** to check host/port and **Participants** to map nicknames to OBS scene names.
6. Click **Test OBS**, then **Start director**, and authorize the new app in TeamSpeak if requested.
7. Have a friend talk and verify the corresponding scene switches. Restart the director after modifying settings.

### Default timing (same as the working Python v3)

| Setting | Default |
|---|---:|
| Switch delay | 500 ms |
| Minimum scene duration | 2,500 ms |
| Return to default after silence | 4,500 ms |
| Merge brief pauses | 700 ms |
| Restrict speakers to same channel | Enabled |
| Verbose logging | Disabled |

Your personal build uses the values from your own config file, including any changes to these defaults.

## Configuration file format

See `config.example.json`. If a ZIP contains only `config.example.json`, the application copies it to `config.json` on first run. The personal build already includes your exact `config.json`. Changes in the GUI are saved to `config.json` with a `.bak` backup.

Only the local director connects to the TeamSpeak and OBS WebSocket services. Video is handled separately by OBS and VDO.Ninja. Microphone audio should generally come from TeamSpeak, not duplicated from the browser sources.

## Security and publishing

- **Never commit or upload** `config.json`, `.teamspeak_api_key`, personal ZIP files, or the `dist/personal/` directory. They are ignored in `.gitignore` as a safeguard, but check before publishing anyway.
- The GitHub release workflow creates **only** the public package from clean source; it never accesses your local personal files.
- OBS with authentication disabled is **not guaranteed to be localhost-only**. Restrict the listener/firewall accordingly. Re-enable OBS authentication for a public live-streaming setup.
- Random VDO.Ninja links are not a substitute for access controls; treat publishing/view links as potentially sensitive.
- The TeamSpeak API key is saved only to a local `.teamspeak_api_key` file. It is never shown in the GUI log.

## GitHub publication

1. Create a new empty GitHub repository, e.g. `speaker-scene-director`.
2. From this source folder run `git init`, `git add .`, and `git status`. Verify there are **no personal files** staged.
3. Commit, set the remote, and push to `main`. Replace `YOUR_USERNAME` in `Cargo.toml` with your actual GitHub username.
4. The GitHub Actions workflow builds/tests the public Windows package on pushes and pull requests.
5. Tag a release such as `v1.0.0` and push the tag. GitHub Actions attaches `SpeakerSceneDirector-Public.zip` to the release.

## Developer notes

- `src/core.rs`: platform-independent voice-to-scene state machine and unit tests
- `src/teamspeak.rs`: TeamSpeak 6 Remote Apps WebSocket client and reconnection
- `src/obs.rs`: OBS WebSocket v5 handshake, optional authentication and scene requests
- `src/ui.rs`: native graphical user interface (egui/eframe)
- `src/config.rs`: JSON schema, validation, loading, and local persistence
- `scripts/build.ps1`: Windows build, public/private release packaging

## Source and compatibility notes

This is a clean Rust implementation of the working Python v3 logic. The new executable has not yet been end-to-end tested against your exact OBS and TeamSpeak installation. Run the OBS test and a local microphone test before putting it into a live broadcast.

MIT licensed. TeamSpeak, OBS Studio, VDO.Ninja and Rust are independent products/projects and are not affiliated with this application.
