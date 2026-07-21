---
version: alpha
name: Psychevo Adaptive Workbench
description: Compact, evidence-led visual system for Psychevo across terminal, Web, Desktop, and Floating surfaces.
colors:
  canvas-dark: "oklch(12.8% 0.006 85)"
  canvas-light: "oklch(98.55% 0.0045 88)"
  canvas-warm: "oklch(97.2% 0.012 88)"
  ink-dark: "oklch(95.5% 0.012 84)"
  ink-light: "oklch(20.2% 0.003 255)"
  ink-warm: "oklch(20.5% 0.018 72)"
  ledger-cyan: "oklch(82% 0.075 198)"
  brass: "oklch(77% 0.095 76)"
  danger: "oklch(74% 0.12 28)"
  terminal-paper: "#d8cdb8"
typography:
  body-md:
    fontFamily: Ubuntu Sans
    fontSize: 15px
    fontWeight: 400
    lineHeight: 1.45
    letterSpacing: 0em
  chrome-md:
    fontFamily: Ubuntu Sans
    fontSize: 0.96rem
    fontWeight: 500
    lineHeight: 1.35
    letterSpacing: 0em
  session-sm:
    fontFamily: Ubuntu Sans
    fontSize: 0.88rem
    fontWeight: 400
    lineHeight: 1.35
    letterSpacing: 0em
  label-sm:
    fontFamily: Ubuntu Sans
    fontSize: 0.84rem
    fontWeight: 650
    lineHeight: 1.2
    letterSpacing: 0em
  utility-xs:
    fontFamily: Ubuntu Sans
    fontSize: 0.78rem
    fontWeight: 500
    lineHeight: 1.2
    letterSpacing: 0em
  code-sm:
    fontFamily: SFMono-Regular
    fontSize: 12px
    fontWeight: 400
    lineHeight: 1.35
    letterSpacing: 0em
rounded:
  xs: 4px
  sm: 6px
  md: 8px
  pill: 999px
spacing:
  micro: 4px
  xs: 6px
  sm: 8px
  md: 12px
  lg: 18px
  transcript-width: 760px
  workbench-column: 840px
  floating-width: 760px
components:
  transcript-user-prompt:
    backgroundColor: "{colors.canvas-dark}"
    textColor: "{colors.ink-dark}"
    typography: "{typography.body-md}"
    rounded: "{rounded.md}"
    padding: 12px
  evidence-row:
    backgroundColor: transparent
    textColor: "{colors.ink-dark}"
    typography: "{typography.session-sm}"
    rounded: "{rounded.xs}"
    padding: 4px
  icon-button:
    backgroundColor: transparent
    textColor: "{colors.ink-dark}"
    typography: "{typography.chrome-md}"
    rounded: "{rounded.sm}"
    size: 32px
  control:
    compactSize: 28px
    defaultSize: 32px
    coarsePointerTarget: 44px
    compactIconSize: 14px
    defaultIconSize: 16px
    focusWidth: 2px
    focusOffset: 2px
    disabledOpacity: 0.55
    pressOffset: 1px
    roundedCompact: "{rounded.xs}"
    roundedDefault: "{rounded.sm}"
  field:
    compactSize: 28px
    defaultSize: 32px
    paddingX: 9px
    paddingY: 7px
    choiceSize: 16px
    rounded: "{rounded.sm}"
    monoFamily: '"SFMono-Regular", "Cascadia Code", "Roboto Mono", Consolas, "Liberation Mono", monospace'
  composer-input:
    backgroundColor: "{colors.canvas-dark}"
    textColor: "{colors.ink-dark}"
    typography: "{typography.body-md}"
    rounded: "{rounded.sm}"
    padding: 12px
  floating-capsule:
    backgroundColor: "#15191a"
    textColor: "#f6f2e7"
    typography: "{typography.body-md}"
    rounded: "{rounded.md}"
    padding: 8px
  switch:
    trackColor: "var(--pevo-switch-track)"
    trackOnColor: "var(--pevo-switch-track-on)"
    thumbColor: "var(--pevo-switch-thumb)"
    focusColor: "var(--pevo-switch-focus)"
    typography: "{typography.label-sm}"
    rounded: "{rounded.pill}"
themes:
  dark:
    colorScheme: dark
    selector: ":root"
    fontFamily: '"Ubuntu Sans", "Aptos", "Segoe UI Variable", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif'
    cssVariables:
      bg: "oklch(12.8% 0.006 85)"
      bg-raised: "oklch(16.5% 0.008 85)"
      panel: "oklch(18.6% 0.009 85)"
      panel-muted: "oklch(22.5% 0.011 85)"
      panel-warm: "oklch(22.5% 0.018 78)"
      sidebar-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 58%)"
      sidebar-border: "oklch(28% 0.012 85)"
      sidebar-active: "oklch(24% 0.011 85)"
      ink: "oklch(95.5% 0.012 84)"
      muted: "oklch(76% 0.012 84)"
      muted-strong: "oklch(84% 0.011 84)"
      faint: "oklch(61% 0.012 84)"
      nav-text: "oklch(91% 0.011 84)"
      nav-muted: "oklch(78% 0.011 84)"
      text: "var(--pevo-ink)"
      text-muted: "var(--pevo-nav-muted)"
      border: "oklch(31% 0.012 84)"
      border-strong: "oklch(44% 0.014 84)"
      user-bubble: "oklch(25.5% 0.012 84)"
      user-bubble-border: "oklch(36% 0.014 84)"
      accent: "oklch(91% 0.014 84)"
      accent-ink: "oklch(13% 0.006 84)"
      accent-soft: "oklch(25.5% 0.014 84)"
      control-primary-bg: "transparent"
      control-primary-ink: "var(--pevo-ink)"
      control-primary-border: "transparent"
      control-interrupt-bg: "oklch(31% 0.008 85)"
      control-interrupt-ink: "var(--pevo-ink)"
      control-interrupt-border: "oklch(31% 0.008 85)"
      control-secondary-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 18%)"
      control-hover-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 38%)"
      control-selected-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 22%)"
      control-focus: "color-mix(in oklch, var(--pevo-switch-track-on), white 24%)"
      brass: "oklch(77% 0.095 76)"
      brass-soft: "oklch(26% 0.038 76)"
      caution: "oklch(80% 0.105 64)"
      danger: "oklch(74% 0.12 28)"
      control-danger-bg: "color-mix(in oklch, var(--pevo-danger), transparent 82%)"
      control-caution-bg: "color-mix(in oklch, var(--pevo-brass), transparent 84%)"
      field-bg: "color-mix(in oklch, var(--pevo-panel), var(--pevo-bg) 34%)"
      field-search-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 52%)"
      field-code-bg: "color-mix(in oklch, var(--pevo-code-bg), var(--pevo-panel) 18%)"
      field-border: "color-mix(in oklch, var(--pevo-border), transparent 10%)"
      field-border-hover: "var(--pevo-border-strong)"
      field-focus: "var(--pevo-control-focus)"
      field-placeholder: "var(--pevo-muted)"
      field-readonly-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 48%)"
      code-bg: "oklch(11.5% 0.008 85)"
      code-ink: "oklch(95% 0.012 84)"
      code-preview-ink: "var(--pevo-code-ink)"
      hl-comment: "oklch(68% 0.012 84)"
      hl-keyword: "oklch(82% 0.085 286)"
      hl-string: "oklch(80% 0.11 150)"
      hl-number: "oklch(82% 0.1 78)"
      hl-function: "oklch(82% 0.075 238)"
      hl-type: "oklch(82% 0.07 198)"
      hl-builtin: "oklch(82% 0.085 38)"
      hl-meta: "oklch(75% 0.035 250)"
      markdown-code-bg: "var(--pevo-code-bg)"
      markdown-code-ink: "var(--pevo-code-ink)"
      markdown-code-border: "color-mix(in oklch, var(--pevo-border), transparent 42%)"
      markdown-inline-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 42%)"
      diff-bg: "var(--pevo-code-bg)"
      diff-ink: "var(--pevo-code-ink)"
      diff-muted: "color-mix(in oklch, var(--pevo-muted), var(--pevo-code-ink) 18%)"
      diff-border: "color-mix(in oklch, var(--pevo-border), transparent 34%)"
      diff-header-bg: "color-mix(in oklch, var(--pevo-code-bg), var(--pevo-panel) 22%)"
      diff-meta-bg: "color-mix(in oklch, var(--pevo-code-bg), var(--pevo-panel) 14%)"
      diff-hunk-bg: "color-mix(in oklch, var(--pevo-code-bg), var(--pevo-panel) 26%)"
      diff-add: "oklch(72% 0.13 150)"
      diff-delete: "oklch(72% 0.13 28)"
      diff-add-bg: "color-mix(in oklch, var(--pevo-diff-add), var(--pevo-code-bg) 86%)"
      diff-delete-bg: "color-mix(in oklch, var(--pevo-diff-delete), var(--pevo-code-bg) 87%)"
      radius: "8px"
      radius-sm: "6px"
      radius-xs: "4px"
      font-size-base: "15px"
      font-size-chrome: "0.96rem"
      font-size-session: "0.88rem"
      font-size-small: "0.84rem"
      font-size-xsmall: "0.78rem"
      shadow: "0 18px 42px rgb(0 0 0 / 18%)"
      shadow-popover: "0 18px 42px rgb(0 0 0 / 24%)"
      shadow-panel: "inset 0 0 0 1px color-mix(in oklch, var(--pevo-border), transparent 32%)"
      shadow-active: "0 1px 0 rgb(255 255 255 / 4%), 0 6px 16px rgb(0 0 0 / 18%)"
      switch-track: "color-mix(in oklch, var(--pevo-panel-muted), var(--pevo-bg) 24%)"
      switch-track-hover: "color-mix(in oklch, var(--pevo-panel-muted), var(--pevo-ink) 8%)"
      switch-track-on: "oklch(64% 0.13 238)"
      switch-border: "color-mix(in oklch, var(--pevo-border), transparent 18%)"
      switch-thumb: "oklch(96% 0.012 84)"
      switch-thumb-on: "#ffffff"
      switch-shadow: "0 1px 2px rgb(0 0 0 / 28%), 0 0 0 1px rgb(0 0 0 / 10%)"
      switch-focus: "color-mix(in oklch, var(--pevo-switch-track-on), white 24%)"
      floating-bg: "#15191a"
      floating-bg-raised: "#273034"
      floating-ink: "#f6f2e7"
      floating-muted: "color-mix(in oklch, #f6f2e7, transparent 55%)"
      floating-border: "color-mix(in oklch, #f6f2e7, transparent 82%)"
      floating-action: "#d7e2f0"
      floating-action-ink: "#11181f"
      floating-chip-bg: "color-mix(in oklch, #090b09, transparent 42%)"
      floating-field-bg: "color-mix(in oklch, #090b09, transparent 24%)"
      floating-hover-bg: "color-mix(in oklch, #f6f2e7, transparent 88%)"
      floating-danger-bg: "color-mix(in oklch, #b85a4a, transparent 74%)"
      floating-running: "#7fd1c4"
      floating-shadow: "0 18px 60px color-mix(in oklch, #000, transparent 45%), 0 2px 8px color-mix(in oklch, #000, transparent 55%)"
  light:
    colorScheme: light
    selector: 'html[data-pevo-appearance="light"]'
    cssVariables:
      bg: "oklch(98.55% 0.0045 88)"
      bg-raised: "oklch(99% 0.0035 88)"
      panel: "oklch(99.35% 0.0028 88)"
      panel-muted: "oklch(95.4% 0.005 88)"
      panel-warm: "oklch(96.4% 0.008 84)"
      sidebar-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 58%)"
      sidebar-border: "oklch(86.6% 0.010 86)"
      sidebar-active: "oklch(98.55% 0.006 88)"
      ink: "oklch(20.2% 0.003 255)"
      muted: "oklch(49% 0.004 255)"
      muted-strong: "oklch(35% 0.004 255)"
      faint: "oklch(65% 0.003 255)"
      nav-text: "oklch(27% 0.003 255)"
      nav-muted: "oklch(45% 0.003 255)"
      text: "var(--pevo-ink)"
      text-muted: "var(--pevo-nav-muted)"
      border: "oklch(88.2% 0.005 86)"
      border-strong: "oklch(77.6% 0.006 86)"
      user-bubble: "oklch(96% 0.005 88)"
      user-bubble-border: "oklch(85.8% 0.006 86)"
      accent: "oklch(42% 0.012 255)"
      accent-ink: "oklch(99.4% 0.001 255)"
      accent-soft: "oklch(94.2% 0.003 255)"
      control-primary-bg: "transparent"
      control-primary-ink: "var(--pevo-ink)"
      control-primary-border: "transparent"
      control-interrupt-bg: "oklch(32% 0.004 255)"
      control-interrupt-ink: "var(--pevo-accent-ink)"
      control-interrupt-border: "oklch(32% 0.004 255)"
      control-secondary-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 8%)"
      control-hover-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 24%)"
      control-selected-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 4%)"
      control-focus: "color-mix(in oklch, var(--pevo-switch-track-on), white 32%)"
      brass: "oklch(58% 0.12 68)"
      brass-soft: "oklch(94.8% 0.036 72)"
      caution: "oklch(50% 0.12 64)"
      danger: "oklch(48% 0.15 28)"
      control-danger-bg: "color-mix(in oklch, var(--pevo-danger), transparent 88%)"
      control-caution-bg: "color-mix(in oklch, var(--pevo-brass), transparent 88%)"
      field-bg: "color-mix(in oklch, var(--pevo-panel), var(--pevo-bg) 26%)"
      field-search-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 52%)"
      field-code-bg: "color-mix(in oklch, var(--pevo-code-bg), var(--pevo-panel) 22%)"
      field-border: "color-mix(in oklch, var(--pevo-border), transparent 8%)"
      field-border-hover: "var(--pevo-border-strong)"
      field-focus: "var(--pevo-control-focus)"
      field-placeholder: "var(--pevo-muted)"
      field-readonly-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 46%)"
      code-bg: "oklch(96.8% 0.0045 88)"
      code-ink: "oklch(21% 0.003 255)"
      code-preview-ink: "var(--pevo-ink)"
      hl-comment: "oklch(49% 0.004 255)"
      hl-keyword: "oklch(43% 0.11 286)"
      hl-string: "oklch(41% 0.105 150)"
      hl-number: "oklch(43% 0.105 72)"
      hl-function: "oklch(42% 0.105 238)"
      hl-type: "oklch(42% 0.095 198)"
      hl-builtin: "oklch(45% 0.11 38)"
      hl-meta: "oklch(45% 0.055 250)"
      markdown-code-bg: "oklch(95.8% 0.005 88)"
      markdown-code-ink: "var(--pevo-ink)"
      markdown-code-border: "oklch(86% 0.006 86)"
      markdown-inline-bg: "oklch(92.8% 0.005 88)"
      diff-bg: "oklch(98.4% 0.003 88)"
      diff-ink: "oklch(21% 0.003 255)"
      diff-muted: "oklch(49% 0.004 255)"
      diff-border: "oklch(86.4% 0.006 86)"
      diff-header-bg: "oklch(94.6% 0.0045 88)"
      diff-meta-bg: "oklch(96.6% 0.004 88)"
      diff-hunk-bg: "oklch(93.6% 0.005 88)"
      diff-add: "oklch(42% 0.13 150)"
      diff-delete: "oklch(47% 0.15 28)"
      diff-add-bg: "oklch(94.2% 0.038 150)"
      diff-delete-bg: "oklch(94.2% 0.036 28)"
      shadow: "0 18px 42px rgb(0 0 0 / 8%)"
      shadow-popover: "0 18px 42px rgb(0 0 0 / 12%)"
      shadow-panel: "0 1px 2px rgb(0 0 0 / 6%), 0 10px 28px rgb(0 0 0 / 4%)"
      shadow-active: "0 1px 3px rgb(0 0 0 / 9%), 0 8px 18px rgb(0 0 0 / 4%)"
      switch-track: "oklch(90.5% 0.006 255)"
      switch-track-hover: "oklch(86.5% 0.010 255)"
      switch-track-on: "oklch(57% 0.16 246)"
      switch-border: "oklch(82% 0.008 255)"
      switch-thumb: "#ffffff"
      switch-thumb-on: "#ffffff"
      switch-shadow: "0 1px 2px rgb(0 0 0 / 18%), 0 0 0 1px rgb(0 0 0 / 4%)"
      switch-focus: "color-mix(in oklch, var(--pevo-switch-track-on), white 32%)"
  warm:
    colorScheme: light
    selector: 'html[data-pevo-appearance="warm"]'
    cssVariables:
      bg: "oklch(97.2% 0.012 88)"
      bg-raised: "oklch(98.8% 0.008 88)"
      panel: "oklch(99.1% 0.006 88)"
      panel-muted: "oklch(94.8% 0.014 88)"
      panel-warm: "oklch(96.3% 0.023 78)"
      sidebar-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 58%)"
      sidebar-border: "oklch(84.8% 0.024 86)"
      sidebar-active: "oklch(98.2% 0.011 88)"
      ink: "oklch(20.5% 0.018 72)"
      muted: "oklch(47% 0.017 72)"
      muted-strong: "oklch(36% 0.018 72)"
      faint: "oklch(63% 0.017 72)"
      nav-text: "oklch(24% 0.018 72)"
      nav-muted: "oklch(43% 0.017 72)"
      text: "var(--pevo-ink)"
      text-muted: "var(--pevo-nav-muted)"
      border: "oklch(86.5% 0.018 86)"
      border-strong: "oklch(75.8% 0.026 84)"
      user-bubble: "oklch(93% 0.017 88)"
      user-bubble-border: "oklch(82.8% 0.024 86)"
      accent: "oklch(46% 0.03 72)"
      accent-ink: "oklch(99% 0.004 90)"
      accent-soft: "oklch(92% 0.026 78)"
      control-primary-bg: "transparent"
      control-primary-ink: "var(--pevo-ink)"
      control-primary-border: "transparent"
      control-interrupt-bg: "oklch(33% 0.018 72)"
      control-interrupt-ink: "var(--pevo-accent-ink)"
      control-interrupt-border: "oklch(33% 0.018 72)"
      control-secondary-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 8%)"
      control-hover-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 24%)"
      control-selected-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 4%)"
      control-focus: "color-mix(in oklch, var(--pevo-switch-track-on), white 32%)"
      brass: "oklch(55% 0.112 72)"
      brass-soft: "oklch(93.8% 0.043 78)"
      caution: "oklch(49% 0.12 64)"
      danger: "oklch(48% 0.15 28)"
      control-danger-bg: "color-mix(in oklch, var(--pevo-danger), transparent 88%)"
      control-caution-bg: "color-mix(in oklch, var(--pevo-brass), transparent 88%)"
      field-bg: "color-mix(in oklch, var(--pevo-panel), var(--pevo-bg) 24%)"
      field-search-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 52%)"
      field-code-bg: "color-mix(in oklch, var(--pevo-code-bg), var(--pevo-panel) 22%)"
      field-border: "color-mix(in oklch, var(--pevo-border), transparent 8%)"
      field-border-hover: "var(--pevo-border-strong)"
      field-focus: "var(--pevo-control-focus)"
      field-placeholder: "var(--pevo-muted)"
      field-readonly-bg: "color-mix(in oklch, var(--pevo-panel-muted), transparent 44%)"
      code-bg: "oklch(96.2% 0.012 88)"
      code-ink: "oklch(22% 0.018 72)"
      code-preview-ink: "var(--pevo-ink)"
      hl-comment: "oklch(48% 0.017 72)"
      hl-keyword: "oklch(43% 0.11 286)"
      hl-string: "oklch(41% 0.105 150)"
      hl-number: "oklch(43% 0.105 72)"
      hl-function: "oklch(42% 0.105 238)"
      hl-type: "oklch(42% 0.095 198)"
      hl-builtin: "oklch(45% 0.11 38)"
      hl-meta: "oklch(45% 0.055 250)"
      markdown-code-bg: "oklch(95.6% 0.013 88)"
      markdown-code-ink: "var(--pevo-ink)"
      markdown-code-border: "oklch(84.8% 0.02 86)"
      markdown-inline-bg: "oklch(91.8% 0.019 88)"
      diff-bg: "oklch(98.2% 0.008 88)"
      diff-ink: "oklch(22% 0.018 72)"
      diff-muted: "oklch(47% 0.017 72)"
      diff-border: "oklch(84.5% 0.019 86)"
      diff-header-bg: "oklch(94.4% 0.014 88)"
      diff-meta-bg: "oklch(96% 0.012 88)"
      diff-hunk-bg: "oklch(92.8% 0.018 88)"
      diff-add: "oklch(42% 0.13 150)"
      diff-delete: "oklch(47% 0.15 28)"
      diff-add-bg: "oklch(94.2% 0.038 150)"
      diff-delete-bg: "oklch(94.2% 0.036 28)"
      shadow: "0 18px 42px rgb(48 38 25 / 10%)"
      shadow-popover: "0 18px 42px rgb(48 38 25 / 14%)"
      shadow-panel: "0 1px 2px rgb(48 38 25 / 7%), 0 10px 28px rgb(48 38 25 / 5%)"
      shadow-active: "0 1px 3px rgb(48 38 25 / 10%), 0 8px 18px rgb(48 38 25 / 5%)"
      switch-track: "color-mix(in oklch, var(--pevo-panel-muted), var(--pevo-border) 18%)"
      switch-track-hover: "color-mix(in oklch, var(--pevo-panel-muted), var(--pevo-ink) 8%)"
      switch-track-on: "oklch(54% 0.12 238)"
      switch-border: "color-mix(in oklch, var(--pevo-border), transparent 8%)"
      switch-thumb: "oklch(99% 0.004 90)"
      switch-thumb-on: "#ffffff"
      switch-shadow: "0 1px 2px rgb(48 38 25 / 18%), 0 0 0 1px rgb(48 38 25 / 4%)"
      switch-focus: "color-mix(in oklch, var(--pevo-switch-track-on), white 32%)"
motion:
  feedback: 120ms
  content: 180ms
  spinnerFrame: 120ms
  reduceMotionDuration: 0ms
glyphs:
  prompt: "›"
  evidence: "•"
  quiet: "·"
  collapsed: "▸"
  expanded: "▾"
platforms:
  web:
    cssVariablePrefix: "--pevo-"
    appearances:
      - dark
      - light
      - warm
  tui:
    roles:
      accent: ansi-cyan
      identity: ansi-magenta
      danger: ansi-red
      success: ansi-green
      dim: ansi-dark-gray
      thinking: "{colors.terminal-paper}"
    fallback:
      surface-bg: "#262626"
      selection-bg: "#3e5869"
  floating:
    scopeClass: pevo-floating
    rootComponent: FloatingApp
    consumes:
      - "--pevo-floating-*"
      - "--pevo-radius-*"
      - "--pevo-font-size-*"
  embeddedTerminal:
    appearances:
      dark:
        background: "#151410"
        foreground: "#f3efe7"
        cursor: "#f3efe7"
        cursorAccent: "#151410"
        selectionBackground: "#3f372d"
        selectionInactiveBackground: "#332d25"
        black: "#5c554b"
        red: "#ff6b6b"
        green: "#7bcf8a"
        yellow: "#d8b85f"
        blue: "#82b1ff"
        magenta: "#d59bf6"
        cyan: "#75d7d0"
        white: "#e8ded0"
        brightBlack: "#8b8173"
        brightRed: "#ff8a8a"
        brightGreen: "#9ee6a8"
        brightYellow: "#f0d987"
        brightBlue: "#a6c8ff"
        brightMagenta: "#e7b7ff"
        brightCyan: "#9cebe5"
        brightWhite: "#fffaf1"
      light:
        background: "#f7f5ef"
        foreground: "#202225"
        cursor: "#202225"
        cursorAccent: "#f7f5ef"
        selectionBackground: "#d8dde5"
        selectionInactiveBackground: "#e5e8ed"
        black: "#202225"
        red: "#a53b3b"
        green: "#2f7d4f"
        yellow: "#8a6400"
        blue: "#245db2"
        magenta: "#8a4fa3"
        cyan: "#227c89"
        white: "#5f6670"
        brightBlack: "#6a6f78"
        brightRed: "#bf4c4c"
        brightGreen: "#388e5d"
        brightYellow: "#a77a00"
        brightBlue: "#2f6ecb"
        brightMagenta: "#9b5eb6"
        brightCyan: "#2d8d9b"
        brightWhite: "#3a3f46"
      warm:
        background: "#f5efe3"
        foreground: "#2d261f"
        cursor: "#2d261f"
        cursorAccent: "#f5efe3"
        selectionBackground: "#eadfce"
        selectionInactiveBackground: "#efe7da"
        black: "#2d261f"
        red: "#9f4238"
        green: "#39764c"
        yellow: "#846217"
        blue: "#2e5f9f"
        magenta: "#7a558f"
        cyan: "#28767c"
        white: "#62584f"
        brightBlack: "#756b61"
        brightRed: "#b75245"
        brightGreen: "#45875a"
        brightYellow: "#9b7420"
        brightBlue: "#3a70b7"
        brightMagenta: "#8b65a3"
        brightCyan: "#32888e"
        brightWhite: "#453d35"
---

# Psychevo Adaptive Workbench

## Overview

Psychevo is a working ledger for agentic software work. The design system
should feel like a calm operations bench: compact, inspectable, and precise
under repetition. Its audience is developers who spend long sessions reading
agent work, controlling active turns, and comparing evidence to outcomes.

The defining trait is Adaptive Evidence. Runtime work appears close to the
answer it supports, starts summarized, and expands only when the user asks for
detail. The UI should never turn every runtime event into a loud activity feed.

## Colors

The palette is ANSI-first in spirit and variable-driven in browser surfaces.
It uses quiet neutral canvases, low-chroma surface steps, and scarce accents.

- **Canvas:** dark, light, and warm canvases are tuned for long reading
  sessions rather than brand spectacle.
- **Ink:** foreground colors use high contrast without pure black or pure
  white as the ordinary reading color.
- **Accent:** cyan is the ordinary action and focus accent in terminal
  surfaces; browser surfaces may express the same role through a muted
  neutral accent when the host appearance demands restraint.
- **Identity:** magenta is reserved for rare identity or mode moments and must
  not become the main theme.
- **Danger:** red marks failure words or bounded error states, not whole
  surfaces unless the surface is itself an error.

## Typography

The core UI voice is utilitarian. Browser shells use Ubuntu Sans with system
fallbacks, tabular numerals, and compact role sizes. Embedded terminal panels
use a monospace stack. Terminal UI uses host terminal typography and should not
simulate browser typography.

Use modest type differences. Transcript, status, controls, and metadata should
communicate hierarchy through placement, tone, and grouping before large size
changes.

## Layout

The layout is column-led and composer-first. Transcript rows sit on a readable
center column; composer, completion, permission, and status surfaces remain
attached to the input workflow. Workbench panels and Desktop/Floating windows
may use different geometry, but they should preserve the same compact hierarchy.

Spacing follows small repeated steps. Use density to reduce scanning cost, not
to pack unrelated controls together.

## Elevation & Depth

Depth is tonal. Prefer background steps, indentation, dim text, and spacing
before borders or shadows. Borders are for actual boundaries: modal edges,
input fields, strong panel separation, and terminal limits.

Floating may use a stronger shadow because it sits over other applications.
That shadow is a host-placement cue, not a card aesthetic for the rest of the
product.

## Shapes

The shape language is compact and squared-off with small radii. The default
radius is 8px or less. Pills are reserved for chips, compact status values,
and circular icon controls.

Do not mix decorative roundness into ledger surfaces. Transcript and evidence
rows should read as text material first.

## Components

Components consume semantic roles from this document instead of inventing
local palettes.

- **Transcript:** passive reading surface. User prompt blocks are quiet and
  role-label free. Assistant answers remain unframed by default.
- **Evidence rows:** inline ledger material with compact titles, collapsed
  raw details, elapsed/status slots, and the shared glyph vocabulary.
- **Composer:** quiet input band with completion, attachment, mode, permission,
  and send/interrupt controls anchored to the writing flow.
- **Pickers and panels:** selection-sheet behavior with compact headers,
  searchable lists, selected-row markers, and contextual footers.
- **Switches:** management-style binary controls for enablement and modes. They
  use generated `--pevo-switch-*` roles, keep labels concise, and do not replace
  checkbox groups used for multi-select or confirmation.
- **Action controls:** visible commands use transparent resting surfaces and
  theme foreground text in every appearance. A local primary command is
  distinguished by order, wording, iconography, border, and weight rather than
  a dark/light inversion. Secondary, ghost, caution, and danger treatments stay
  quiet. The Composer interrupt control uses the dedicated neutral deep-gray
  interrupt role because stopping active work is not a danger-state warning.
  Icon-only commands are reserved for familiar chrome and always keep a stable
  accessible name and tooltip.
- **Fields:** search/filter, ordinary value, secret/high-entropy, multiline,
  structured, and editor inputs use explicit semantic variants. Shared field
  roles own color, border, placeholder, focus, read-only, invalid, and disabled
  presentation; product surfaces own widths and specialized editor geometry.
  Checkbox and radio choices never inherit text-field geometry.
- **Navigation and selection:** current browser navigation rows use a quiet
  tonal step without a leading glyph indicator. Tabs change content panes,
  segmented controls choose one value, toggles expose pressed state, and
  disclosures expose expanded state; they do not share a generic active
  presentation contract.
- **Mutation receipts:** committed mutations produce a compact `•` ledger row
  with a plain past-tense result. A reliable inverse may add Undo. Receipts are
  transient display-only feedback rather than floating decorative toast cards.
- **Dialogs and menus:** modal work owns initial and return focus, Escape,
  dismissal, and pending behavior. Menus use one roving tab stop, direction
  keys, Home/End, typeahead, outside dismissal, and focus return.
- **Markdown frontmatter:** document-start YAML metadata renders as a compact
  table before the Markdown body. It uses the shared Markdown table/code
  treatment and existing `--pevo-*` border, panel, code, and ink roles; scalar
  arrays may use small chips, while nested values stay bounded and code-like.
  Preview surfaces may add a quiet icon-only copy action that stays visually
  subordinate to the document and copies raw Markdown through the host clipboard.
- **Floating:** scoped visual surface that uses shared tokens while keeping
  all selectors under `.pevo-floating`.
- **Embedded terminal:** uses the terminal palette in `platforms.embeddedTerminal`
  rather than isolated hardcoded colors.

## Do's and Don'ts

- Do keep evidence close to the answer it supports.
- Do make controls authoritative for their current value instead of repeating
  labels, chips, and helper text.
- Do use generated `--pevo-*` variables for browser color, radius, typography,
  shadow, and Floating token roles.
- Do keep TUI roles readable under ANSI16, ANSI256, and truecolor terminals.
- Don't add decorative borders, nested cards, gradient backgrounds, or hero
  moments to workbench surfaces.
- Don't use magenta as the ordinary focus/action color.
- Don't introduce unscoped package CSS into Desktop or Workbench.
- Don't add hidden visual modes that require memorized shortcuts to understand.
