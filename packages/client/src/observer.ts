export type ObserverDiagnosticSource =
  | "capabilities_listener"
  | "thread_session_listener";

export interface ObserverDiagnostic {
  message: string;
  source: ObserverDiagnosticSource;
}

export type ObserverDiagnosticHandler = (diagnostic: ObserverDiagnostic) => void;

const MAX_DIAGNOSTIC_MESSAGE_LENGTH = 1_000;

export function notifyObservers<T>(
  observers: Iterable<T>,
  notify: (observer: T) => void,
  source: ObserverDiagnosticSource,
  reportDiagnostic: ObserverDiagnosticHandler = reportConsoleDiagnostic
): void {
  for (const observer of observers) {
    try {
      notify(observer);
    } catch (error) {
      const message = (error instanceof Error ? error.message : String(error))
        .slice(0, MAX_DIAGNOSTIC_MESSAGE_LENGTH);
      try {
        reportDiagnostic({ message, source });
      } catch {
        // A diagnostic sink cannot become a recursive client failure path.
      }
    }
  }
}

function reportConsoleDiagnostic(diagnostic: ObserverDiagnostic): void {
  console.error(`[psychevo/client] ${diagnostic.source}: ${diagnostic.message}`);
}
