# TUI Troubleshooting

## Mouse input does not reach the TUI

**Symptoms:** Mouse wheel scrolling does not move the transcript, fullscreen shows host terminal scrollback, wheel input appears to recall prompt history, or mouse clicks and selection do not work inside tmux.

**Cause:** The terminal environment is not forwarding mouse events to the fullscreen TUI. This usually means terminal Mouse Reporting is disabled, or tmux mouse mode is off. When wheel input is not delivered as a mouse event with coordinates, the TUI cannot route it by hover area.

**Fix:** Enable Mouse Reporting in the terminal. If the TUI runs inside tmux, add `set -g mouse on` to `.tmux.conf`, or run `tmux set -g mouse on` for the current tmux server. Re-enter fullscreen TUI after changing the setting.

## Text highlights but does not reach the local clipboard

**Symptoms:** Drag selection highlights text inside fullscreen `pevo tui`, but
the selected text is not available from the local terminal clipboard after mouse
release or `Ctrl+C`.

**Cause:** The TUI is receiving mouse input, but the clipboard transfer path is
blocked. Over SSH, local clipboard copy depends on terminal-mediated OSC52
forwarding, or tmux clipboard forwarding when running inside tmux.

**Fix:** In iTerm2, enable clipboard access for terminal applications. If the
TUI runs inside tmux, ensure tmux clipboard forwarding is not disabled, then
re-enter fullscreen TUI. Native remote clipboard tools such as `xclip`,
`wl-copy`, or `pbcopy` copy on the remote host and are not sufficient for SSH
clipboard transfer.
