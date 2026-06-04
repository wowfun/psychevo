Require bash
Require python3
Set Shell bash
Set Width 1200
Set Height 720
Set FontSize 18
Env TERM "xterm-256color"
Env COLORTERM "truecolor"
Env CLICOLOR_FORCE "1"
Env PSYCHEVO_HOME {{PSYCHEVO_HOME}}
Env PSYCHEVO_DB {{PSYCHEVO_DB}}
Env PSYCHEVO_CONFIG {{PSYCHEVO_CONFIG}}
Env TEST_PROVIDER_KEY "test-key"
Type {{PEVO_CMD}}
Enter
Wait+Screen /Ask pevo/
Type "/model"
Enter
Wait+Screen /Add provider/
Sleep 500 ms
Screenshot "01-model-picker.png"
Escape
Sleep 100 ms
Type "/diff"
Enter
Wait+Screen /D I F F/
Wait+Screen /diff-demo.rs/
Sleep 300 ms
Screenshot "19-diff-overlay.png"
Sleep 500 ms
Escape
Sleep 100 ms
Type "Inline edit diff VHS fixture"
Enter
Wait+Screen /Edited inline-diff-fixture.txt/
Wait+Screen /line 02: limit = 2000/
Wait+Screen /INLINE_EDIT_DIFF_FINAL/
Sleep 300 ms
Screenshot "20-inline-edit-diff.png"
Sleep 200 ms
Type "/new"
Enter
Wait+Screen /Ask pevo/
Sleep 100 ms
Type "Permission approval VHS fixture"
Enter
Wait+Screen /Permission required/
Wait+Screen /\/etc\/hosts/
Sleep 300 ms
Screenshot "21-permission-approval.png"
Type "y"
Wait+Screen /PERMISSION_APPROVAL_FINAL/
Type "/new"
Enter
Wait+Screen /Ask pevo/
Sleep 100 ms
Type "Inspect the snapshot harness and read fixture.txt"
Enter
Wait+Screen /exec_command sleep 2 && cat fixture.txt/
Sleep 200 ms
Screenshot "02-running-thinking.png"
Wait+Screen /SNAPSHOT_DEMO_FINAL/
Sleep 300 ms
Ctrl+B
Sleep 300 ms
Screenshot "03-final-ledger.png"
Escape
Sleep 100 ms
Type "!"
Sleep 200 ms
Screenshot "04-shell-mode.png"
Escape
Sleep 200 ms
Type "Long markdown bottom scroll fixture"
Enter
Wait+Screen /LONG_MARKDOWN_BOTTOM_MARKER/
Sleep 300 ms
PageUp 8
Sleep 100 ms
PageDown 40
Wait+Screen /LONG_MARKDOWN_BOTTOM_MARKER/
Sleep 300 ms
Screenshot "05-long-markdown-bottom-scroll.png"
Sleep 200 ms
Type "/new"
Enter
Wait+Screen /Ask pevo/
Sleep 200 ms
Type "Reasoning-only table bottom scroll fixture"
Enter
Wait+Screen /REASONING_ONLY_BOTTOM_MARKER/
Sleep 300 ms
Screenshot "06-reasoning-only-collapsed.png"
PageUp 8
Sleep 100 ms
PageDown 80
Wait+Screen /REASONING_ONLY_BOTTOM_MARKER/
Sleep 300 ms
Screenshot "07-reasoning-only-bottom-scroll.png"
Type "Visible write preamble fixture"
Enter
Wait+Screen /Now I have all the data needed/
Sleep 300 ms
Screenshot "08-visible-write-preamble.png"
Wait+Screen /VISIBLE_WRITE_FINAL/
Sleep 200 ms
Type "Interrupted exec command fixture"
Enter
Wait+Screen /exec_command sleep 60/
Sleep 300 ms
Escape
Wait+Screen /interrupted/
Sleep 300 ms
Screenshot "09-interrupted-exec-command.png"
Sleep 200 ms
Type "/new"
Enter
Wait+Screen /Ask pevo/
Sleep 200 ms
Type "Clarify VHS fixture"
Enter
Wait+Screen /Question 1\/1 \(1 unanswered\)/
Sleep 300 ms
Screenshot "16-clarify-panel.png"
Down 2
Enter
Type "Use OAuth credentials"
Sleep 300 ms
Screenshot "17-clarify-other-inline.png"
Enter
Wait+Screen /Questions 1\/1 answered/
Sleep 300 ms
Screenshot "18-clarify-result.png"
Wait+Screen /SNAPSHOT_DEMO_FINAL/
Type "/new"
Enter
Wait+Screen /Ask pevo/
Sleep 200 ms
Type "Subagent foreground VHS fixture"
Enter
Wait+Screen /translate\(Translate user message to Chinese\)/
Sleep 300 ms
Screenshot "10-agent-tool-running.png"
Ctrl+T
Enter
Wait+Screen /Checking terminology/
Sleep 300 ms
Screenshot "11-agent-session-running.png"
Sleep 5600 ms
Alt+P
Wait+Screen /Translation complete/
Sleep 300 ms
Screenshot "12-agent-parent-completed.png"
Type "/agents"
Enter
Wait+Screen /No running subagents/
Sleep 300 ms
Screenshot "12-agents-running.png"
Tab
Wait+Screen /Shadowed duplicates/
Sleep 300 ms
Screenshot "13-agents-available.png"
Down
Down
Enter
Wait+Screen /Start a background fresh-context child run/
Sleep 300 ms
Screenshot "14-agent-actions.png"
Down
Enter
Wait+Screen /Run Agent/
Sleep 300 ms
Screenshot "15-agent-run-prompt.png"
Sleep 500 ms
Escape
Escape
Ctrl+D
