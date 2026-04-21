# Pester

Pester is a cross-platform reminder daemon. It sends native desktop
notifications at configured daily times and keeps sending them at a repeat
interval until you explicitly mark the reminder done for the current day.

It is designed for reminders that should not be easy to ignore:

```sh
pester add winddown --time 22:00 --every 5m --title "Wind down" --message "No exciting stuff now."
pester done winddown
```

## Install

Linux and macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/aloglu/pester/main/install.sh | sh
```

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/aloglu/pester/main/install.ps1 | iex
```

The installer detects the operating system and CPU architecture, downloads the
matching GitHub Release artifact, verifies its checksum, installs Pester for the
current user, installs the background service, and starts it.

Pester does not require Rust, Python, Node.js, Java, Docker, or an external
notification command at runtime.

## Examples

Add a bedtime wind-down reminder:

```sh
pester add winddown --time 22:00 --every 5m --title "Wind down" --message "No exciting stuff now."
```

By default, reminders repeat from their scheduled time until local midnight.
For reminders late in the day, make the reminder window explicit:

```sh
pester add winddown --time 23:50 --every 5m --until 03:00 --title "Wind down" --message "No exciting stuff now."
pester add stretch --time 14:00 --every 10m --for 1h --title "Stretch" --message "Stand up and stretch."
pester add meds --time 09:00 --every 5m --max 3 --title "Medication" --message "Take morning medication."
```

Add medication reminders:

```sh
pester add meds-afternoon --time 14:00 --every 5m --title "Medication" --message "Take afternoon medication."
pester add meds-evening --time 20:00 --every 5m --title "Medication" --message "Take evening medication."
```

Mark one reminder done for today:

```sh
pester done winddown
```

Mark every reminder done for today:

```sh
pester done all
```

Change a reminder:

```sh
pester set winddown --time 23:00
pester set winddown --every 10m
pester set winddown --until 03:00
pester set winddown --clear-until
pester set winddown --message "Start winding down."
```

Temporarily disable or re-enable reminders:

```sh
pester disable winddown
pester enable winddown
pester disable all
pester enable all
```

List reminders and inspect status:

```sh
pester list
pester status
pester doctor
```

Send a test notification:

```sh
pester test winddown
```

## Confirmation

Pester requires full-word confirmations. Single-letter confirmations such as
`y` and `n` are not accepted.

Commands that mark reminders done use `yes` by default:

```text
Mark "winddown" done for today?
Type yes or no:
```

You can set a custom confirmation phrase for `done` commands:

```sh
pester confirm done set
```

Pester will prompt for the phrase interactively. This avoids shell quoting issues
for punctuation, apostrophes, or quotation marks.

You can also pass the phrase directly:

```sh
pester confirm done set "I am a lazy person who shouldn't cancel their reminders."
```

After that, `pester done <id>` and `pester done all` require the exact phrase
instead of `yes`. The typed confirmation does not need surrounding quotes.

Show or reset the custom phrase:

```sh
pester confirm done show
pester confirm done reset
```

Resetting the phrase requires confirmation.

## Commands

```text
pester add <id> --time HH:MM --every 5m --title <title> --message <message> [--until HH:MM] [--for 2h] [--max 3]
pester set <id> [--time HH:MM] [--every 10m] [--until HH:MM] [--for 2h] [--max 3] [--clear-until] [--clear-for] [--clear-max] [--title <title>] [--message <message>]
pester done <id>
pester done all
pester enable <id>
pester enable all
pester disable <id>
pester disable all
pester remove <id>
pester list
pester status
pester test <id>
pester doctor
pester install
pester uninstall
pester uninstall --delete-data
pester daemon
```

Reminder ids may contain ASCII letters, numbers, hyphens, and underscores.
Reserved command words such as `all`, `done`, `enable`, and `disable` cannot be
used as reminder ids.

Times are local 24-hour wall-clock times in `HH:MM` format. By default, a
reminder can notify from its scheduled time until local midnight. `--until`
sets an explicit end time; if the end time is earlier than the scheduled time,
the reminder window continues past midnight into the next day. `--for` sets a
duration shorter than 24 hours. `--max` limits the number of notifications in
one reminder window and can be combined with `--until` or `--for`.

A reminder marked done is done only for the current reminder window. For a
window that crosses midnight, such as `--time 23:50 --until 03:00`, marking it
done after midnight stops the reminder until the next 23:50 window.

## Platform Behavior

### Linux

Pester uses the Freedesktop notification service over the user D-Bus session.
It does not shell out to `notify-send`.

The installer creates a user-level systemd service:

```text
~/.config/systemd/user/pester.service
```

The binary is installed to:

```text
~/.local/bin/pester
```

Desktop environments such as GNOME, KDE Plasma, XFCE, and Cinnamon usually
provide a notification service. Minimal window manager setups may need a
notification daemon such as `dunst` or `mako`.

### macOS

Pester uses the UserNotifications framework.

Pester currently ships for Apple Silicon macOS only.

The installer installs:

```text
~/.local/bin/pester
~/Applications/Pester.app
~/Library/LaunchAgents/com.aloglu.pester.plist
```

The LaunchAgent runs the executable inside `Pester.app` so macOS has a stable
app identity for notification permissions.

### Windows

Pester uses Windows Toast notifications.

The installer installs:

```text
%LOCALAPPDATA%\Programs\Pester\pester.exe
```

It also creates:

```text
Scheduled Task: Pester, or a Startup shortcut fallback if Task Scheduler denies access
Start Menu shortcut: Pester
AppUserModelID: com.aloglu.pester
```

The Start Menu shortcut gives the unpackaged desktop app a stable Toast
notification identity.

### WSL

Pester is designed to send notifications through the operating system's native
notification system. Running the Linux build inside WSL does not automatically
provide access to Windows Toast notifications. Unless the WSL environment has a
working Freedesktop notification bridge, `pester doctor` may report
notifications as unavailable.

For Windows notifications, install and run the Windows build of Pester from
PowerShell.

## Troubleshooting

Run:

```sh
pester doctor
```

`doctor` reports:

- config path
- state path
- current binary path
- notification backend status
- background service status
- platform-specific install details

If notifications are unavailable on Linux, check that a user D-Bus session and a
Freedesktop notification service are running.

If notifications are unavailable on macOS, check System Settings -> Notifications
and look for Pester.

If notifications are unavailable on Windows, check Focus / Do Not Disturb,
notification settings, and whether the Pester Start Menu shortcut exists.

If `pester` is not found after installation, ensure `~/.local/bin` is in `PATH`
on Linux/macOS. The background service uses absolute paths and does not depend
on shell `PATH`.

## Uninstall

Remove Pester but keep reminders and state:

```sh
pester uninstall
```

Remove Pester and delete all reminders/state:

```sh
pester uninstall --delete-data
```

`uninstall --delete-data` requires typing `delete`.

## Release Artifacts

GitHub Releases provide prebuilt artifacts for:

```text
pester-linux-x86_64.tar.gz
pester-linux-aarch64.tar.gz
pester-macos-aarch64.tar.gz
pester-windows-x86_64.zip
pester-windows-aarch64.zip
checksums.txt
```

Users normally do not download these artifacts manually. The install scripts
select the correct artifact automatically.

## License

Released under the [MIT License](https://github.com/aloglu/pester/blob/main/LICENSE).
