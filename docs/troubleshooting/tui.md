# TUI Troubleshooting

## Mouse input does not reach the TUI

**Symptoms:** Mouse wheel scrolling does not move the transcript, fullscreen shows host terminal scrollback, wheel input appears to recall prompt history, or mouse clicks and selection do not work inside tmux.

**Cause:** The terminal environment is not forwarding mouse events to the fullscreen TUI. This usually means terminal Mouse Reporting is disabled, or tmux mouse mode is off. When wheel input is not delivered as a mouse event with coordinates, the TUI cannot route it by hover area.

**Fix:** Enable Mouse Reporting in the terminal. If the TUI runs inside tmux, add `set -g mouse on` to `.tmux.conf`, or run `tmux set -g mouse on` for the current tmux server. Re-enter fullscreen TUI after changing the setting.
