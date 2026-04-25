# pester

pester is a reminder daemon that keeps reminding you until you mark things done.

It sends native desktop notifications at configured daily times and keeps
sending them at a repeat interval until you explicitly mark the reminder done
for the current day.

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

The installers detect the operating system and CPU architecture, download the
matching GitHub Release artifact, verify its checksum, install pester for the
current user, install the background service, and start it.

For Windows, see the [Windows](#windows) section below.

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
pester done --all
```

Mark a reminder not done for today:

```sh
pester undone winddown
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
pester disable --all
pester enable --all
```

Show reminders and inspect system status:

```sh
pester show --all
pester show winddown
pester system status --verbose
```

Show the installed version:

```sh
pester version
```

Send a test notification:

```sh
pester test winddown
```

## Confirmation

pester requires full-word confirmations. Single-letter confirmations such as
`y` and `n` are not accepted.

Commands that mark reminders done use `yes` by default:

```text
Mark "winddown" done for today?
Type yes or no:
```

You can set a custom confirmation phrase for `done` commands:

```sh
pester confirm set
```

pester will prompt for the phrase interactively. This avoids shell quoting issues
for punctuation, apostrophes, or quotation marks.

You can also pass the phrase directly with `--phrase`:

```sh
pester confirm set --phrase "I am a lazy person who shouldn't cancel their reminders."
```

After that, `pester done <id>` and `pester done --all` require the exact phrase
instead of `yes`. The typed confirmation does not need surrounding quotes.

Set a phrase for a specific reminder:

```sh
pester confirm set meds
pester confirm set meds --phrase "I took my medication."
```

Reminder-specific phrases override the global phrase. Show or reset phrases:

```sh
pester confirm show
pester confirm show meds
pester confirm reset
pester confirm reset meds
```

Resetting the phrase requires confirmation.

## Commands

```text
pester add <id> --time HH:MM --every 5m --title <title> --message <message> [--until HH:MM] [--for 2h] [--max 3]
pester set <id> [--time HH:MM] [--every 10m] [--until HH:MM] [--for 2h] [--max 3] [--clear-until] [--clear-for] [--clear-max] [--title <title>] [--message <message>]
pester remove <id> | --all
pester show <id> | --all
pester test <id> | --all
pester done <id> | --all
pester undone <id> | --all
pester enable <id> | --all
pester disable <id> | --all
pester confirm set [<id>] [--phrase <phrase>]
pester confirm show [<id>]
pester confirm reset [<id>]
pester version
pester system status [--verbose]
pester system install
pester system uninstall [--delete-data]
pester system daemon
```

Reminder ids may contain ASCII letters, numbers, hyphens, and underscores.
Names used by pester commands and subcommands are reserved and cannot be used
as reminder ids.

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

pester uses the Freedesktop notification service over the user D-Bus session.
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

pester uses the UserNotifications framework.

pester currently ships for Apple Silicon macOS only.

The installer installs:

```text
~/.local/bin/pester
~/Applications/pester.app
~/Library/LaunchAgents/com.aloglu.pester.plist
```

The LaunchAgent runs the executable inside `pester.app` so macOS has a stable
app identity for notification permissions.

### Windows

> [!WARNING]
> As of April 25, 2026, Windows support has ended with `v0.1.8`.
> `main` and releases after `v0.1.8` are not intended for Windows.
> The last supported installer command is:
>
> ```powershell
> irm https://raw.githubusercontent.com/aloglu/pester/v0.1.8/install.ps1 | iex
> ```

The `v0.1.8` Windows build uses Windows Toast notifications.

The installer installs:

```text
%LOCALAPPDATA%\Programs\pester\pester.exe
%LOCALAPPDATA%\Programs\pester\pesterd.exe
```

It also creates:

```text
Login startup: HKCU\Software\Microsoft\Windows\CurrentVersion\Run\pester
Start Menu shortcut: pester
AppUserModelID: com.aloglu.pester
```

The Start Menu shortcut gives the unpackaged desktop app a stable Toast
notification identity.

### WSL

pester is designed to send notifications through the operating system's native
notification system. Running the Linux build inside WSL does not automatically
provide access to Windows Toast notifications. Unless the WSL environment has a
working Freedesktop notification bridge, `pester system status --verbose` may
report notifications as unavailable.

For Windows notifications, install and run the Windows build of pester from
PowerShell.

## Troubleshooting

Run:

```sh
pester system status --verbose
```

`system status --verbose` reports:

- config path
- state path
- current binary path
- notification backend status
- background service status
- platform-specific install details

If notifications are unavailable on Linux, check that a user D-Bus session and a
Freedesktop notification service are running.

If notifications are unavailable on macOS, check System Settings -> Notifications
and look for pester.

If notifications are unavailable on Windows `v0.1.8`, check Focus / Do Not
Disturb, notification settings, and whether the pester Start Menu shortcut
exists.

If `pester` is not found after installation, ensure `~/.local/bin` is in `PATH`
on Linux/macOS. The background service uses absolute paths and does not depend
on shell `PATH`.

## Uninstall

Remove pester but keep reminders and state:

```sh
pester system uninstall
```

Remove pester and delete all reminders/state:

```sh
pester system uninstall --delete-data
```

`system uninstall --delete-data` requires typing `delete`.

## Release Artifacts

GitHub Releases provide prebuilt artifacts for:

```text
pester-linux-x86_64.tar.gz
pester-linux-aarch64.tar.gz
pester-macos-aarch64.tar.gz
checksums.txt
```

Users normally do not download these artifacts manually. The install scripts
select the correct artifact automatically.

Windows artifacts remain available on the `v0.1.8` release:

```text
pester-windows-x86_64.zip
pester-windows-aarch64.zip
```

## License

Released under the [MIT License](https://github.com/aloglu/pester/blob/main/LICENSE).
