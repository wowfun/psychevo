import { useEffect, useRef, useState } from "react";
import type { Terminal as XTermTerminal, ITheme } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import type { GatewayClient } from "@psychevo/client";
import type { GatewayRequestScope } from "@psychevo/protocol";
import type { Appearance, TerminalNotificationEvent } from "../types";

export function TerminalPanel({
  appearance,
  client,
  scope,
  terminalEvents,
  cwd
}: {
  appearance: Appearance;
  client: GatewayClient | null;
  scope: GatewayRequestScope | null;
  terminalEvents: TerminalNotificationEvent[];
  cwd: string;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<XTermTerminal | null>(null);
  const fitRef = useRef<{ fit(): void } | null>(null);
  const terminalIdRef = useRef<string | null>(null);
  const lastEventSeqRef = useRef(0);
  const [terminalId, setTerminalId] = useState<string | null>(null);
  const [state, setState] = useState<"starting" | "running" | "exited" | "error">("starting");
  const [message, setMessage] = useState("Starting terminal...");

  useEffect(() => {
    if (!client || !scope || !containerRef.current) {
      setState("error");
      setMessage("Terminal is unavailable until the gateway is connected.");
      return;
    }
    let cancelled = false;
    let dataDisposable: { dispose(): void } | null = null;
    let resizeObserver: ResizeObserver | null = null;
    void Promise.all([
      import("@xterm/xterm"),
      import("@xterm/addon-fit")
    ]).then(([xterm, fitModule]) => {
      if (cancelled || !containerRef.current) {
        return;
      }
      const terminal = new xterm.Terminal({
        allowProposedApi: false,
        convertEol: true,
        cursorBlink: true,
        fontFamily: '"SFMono-Regular", "Cascadia Code", "Roboto Mono", monospace',
        fontSize: 12,
        scrollback: 4000,
        theme: terminalTheme(appearance)
      });
      const fit = new fitModule.FitAddon();
      terminal.loadAddon(fit);
      terminal.open(containerRef.current);
      fit.fit();
      terminalRef.current = terminal;
      fitRef.current = fit;
      dataDisposable = terminal.onData((data) => {
        const id = terminalIdRef.current;
        if (!id) {
          return;
        }
        void client.request("terminal/write", {
          terminalId: id,
          dataBase64: bytesToBase64(new TextEncoder().encode(data))
        }).catch(() => {
          setState("error");
          setMessage("Terminal write failed.");
        });
      });
      resizeObserver = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => {
        fit.fit();
        const id = terminalIdRef.current;
        if (id) {
          void client.request("terminal/resize", {
            terminalId: id,
            cols: terminal.cols,
            rows: terminal.rows
          }).catch(() => {});
        }
      });
      resizeObserver?.observe(containerRef.current);
      void client.request("terminal/start", {
        scope,
        cwd: null,
        cols: terminal.cols || 80,
        rows: terminal.rows || 24
      }).then((result) => {
        if (cancelled) {
          void client.request("terminal/terminate", { terminalId: result.terminalId }).catch(() => {});
          return;
        }
        terminalIdRef.current = result.terminalId;
        setTerminalId(result.terminalId);
        setState("running");
        setMessage(result.cwd);
        terminal.focus();
      }).catch((error) => {
        setState("error");
        setMessage(error instanceof Error ? error.message : String(error));
      });
    }).catch((error) => {
      setState("error");
      setMessage(error instanceof Error ? error.message : String(error));
    });
    return () => {
      cancelled = true;
      resizeObserver?.disconnect();
      dataDisposable?.dispose();
      terminalRef.current?.dispose();
      terminalRef.current = null;
      fitRef.current = null;
      const id = terminalIdRef.current;
      terminalIdRef.current = null;
      if (id) {
        void client.request("terminal/terminate", { terminalId: id }).catch(() => {});
      }
    };
  }, [client, scope?.cwd]);

  useEffect(() => {
    const terminal = terminalRef.current;
    if (terminal) {
      terminal.options.theme = terminalTheme(appearance);
    }
  }, [appearance]);

  useEffect(() => {
    const terminal = terminalRef.current;
    const id = terminalIdRef.current;
    if (!terminal || !id) {
      return;
    }
    for (const event of terminalEvents) {
      if (event.seq <= lastEventSeqRef.current) {
        continue;
      }
      if (event.params.terminalId !== id) {
        continue;
      }
      if (event.method === "terminal/output") {
        terminal.write(base64ToBytes(event.params.dataBase64));
      } else {
        setState("exited");
        setMessage(event.params.reason || "exited");
      }
      lastEventSeqRef.current = event.seq;
    }
  }, [terminalEvents, terminalId]);

  return (
    <section className="terminalPanel" aria-label="Terminal">
      <div className="terminalViewport" ref={containerRef}>
        {state !== "running" && <div className={`terminalOverlay is-${state}`}>{message}</div>}
      </div>
    </section>
  );
}

function terminalTheme(appearance: Appearance): ITheme {
  if (appearance === "light") {
    return {
      ...LIGHT_TERMINAL_THEME
    };
  }
  if (appearance === "warm") {
    return {
      ...WARM_TERMINAL_THEME
    };
  }
  return {
    ...DARK_TERMINAL_THEME
  };
}

const DARK_TERMINAL_THEME: ITheme = {
  background: "#151410",
  foreground: "#f3efe7",
  cursor: "#f3efe7",
  cursorAccent: "#151410",
  selectionBackground: "#3f372d",
  selectionInactiveBackground: "#332d25",
  black: "#5c554b",
  red: "#ff6b6b",
  green: "#7bcf8a",
  yellow: "#d8b85f",
  blue: "#82b1ff",
  magenta: "#d59bf6",
  cyan: "#75d7d0",
  white: "#e8ded0",
  brightBlack: "#8b8173",
  brightRed: "#ff8a8a",
  brightGreen: "#9ee6a8",
  brightYellow: "#f0d987",
  brightBlue: "#a6c8ff",
  brightMagenta: "#e7b7ff",
  brightCyan: "#9cebe5",
  brightWhite: "#fffaf1"
};

const LIGHT_TERMINAL_THEME: ITheme = {
  background: "#f7f5ef",
  foreground: "#202225",
  cursor: "#202225",
  cursorAccent: "#f7f5ef",
  selectionBackground: "#d8dde5",
  selectionInactiveBackground: "#e5e8ed",
  black: "#202225",
  red: "#a53b3b",
  green: "#2f7d4f",
  yellow: "#8a6400",
  blue: "#245db2",
  magenta: "#8a4fa3",
  cyan: "#227c89",
  white: "#5f6670",
  brightBlack: "#6a6f78",
  brightRed: "#bf4c4c",
  brightGreen: "#388e5d",
  brightYellow: "#a77a00",
  brightBlue: "#2f6ecb",
  brightMagenta: "#9b5eb6",
  brightCyan: "#2d8d9b",
  brightWhite: "#3a3f46"
};

const WARM_TERMINAL_THEME: ITheme = {
  background: "#f5efe3",
  foreground: "#2d261f",
  cursor: "#2d261f",
  cursorAccent: "#f5efe3",
  selectionBackground: "#eadfce",
  selectionInactiveBackground: "#efe7da",
  black: "#2d261f",
  red: "#9f4238",
  green: "#39764c",
  yellow: "#846217",
  blue: "#2e5f9f",
  magenta: "#7a558f",
  cyan: "#28767c",
  white: "#62584f",
  brightBlack: "#756b61",
  brightRed: "#b75245",
  brightGreen: "#45875a",
  brightYellow: "#9b7420",
  brightBlue: "#3a70b7",
  brightMagenta: "#8b65a3",
  brightCyan: "#32888e",
  brightWhite: "#453d35"
};

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let index = 0; index < bytes.length; index += 1) {
    binary += String.fromCharCode(bytes[index] ?? 0);
  }
  return window.btoa(binary);
}

function base64ToBytes(value: string): Uint8Array {
  const binary = window.atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}
