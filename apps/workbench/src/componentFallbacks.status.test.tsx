// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { HistoryPanel, StatusPanel, TranscriptPanel } from "@psychevo/components";
import type { TranscriptBlock } from "@psychevo/protocol";
import {
  noop,
  sessionSummary,
  setupComponentFallbackTests,
  transcriptBlock,
  transcriptEntry
} from "./componentFallbacks.test-support";

setupComponentFallbackTests();

describe("component fallback rendering", () => {
  it("renders partial settings and missing activity as idle status", () => {
    const html = renderToStaticMarkup(
      <StatusPanel
        sessionId="thread-status"
        status="connected"
        onRefresh={noop}
      />
    );

    expect(html).toContain("thread-status");
    expect(html).toContain("pevo-statusSessionId");
    expect(html).not.toContain("pevo-statusMetric");
    expect(html).toContain("No active context");
    expect(html).toContain("No changes");
  });

  it("renders status usage with a single prompt token disclosure", () => {
    const html = renderToStaticMarkup(
      <StatusPanel
        sessionId="thread-status"
        status="connected"
        context={{
          available: true,
          label: "200/1.0k (20.0%)",
          status: "provider_usage",
          usedTokens: 200,
          contextLimit: 1000,
          percent: 20,
          categories: [
            {
              id: "developer_prompt",
              label: "Developer prompt",
              tokens: 60,
              estimated: true,
              status: "estimated",
              percent: 6,
              details: { skill_entries: [{ name: "design", tokens: 42 }] }
            },
            {
              id: "history",
              label: "History",
              tokens: 140,
              estimated: true,
              status: "estimated",
              percent: 14,
              details: { roles: { user: { count: 1, tokens: 50 } } }
            }
          ],
          advice: []
        }}
        usage={{
          available: true,
          sessionId: "thread-status",
          provider: "mock",
          model: "mock-model",
          messageCount: 2,
          assistantMessageCount: 1,
          contextInputTokens: 200,
          billableInputTokens: 150,
          billableOutputTokens: 50,
          reasoningTokens: 12,
          cacheReadTokens: 80,
          cacheWriteTokens: 10,
          reportedTotalTokens: 250,
          estimatedCostNanodollars: 10_000_000,
          costStatus: "estimated",
          estimatedPricingCount: 1,
          freePricingCount: 0,
          includedPricingCount: 0,
          unknownPricingCount: 0,
          cacheReadPercent: 40
        }}
        onRefresh={noop}
      />
    );

    expect(html).toContain("pevo-promptTokenStack");
    expect(html).toContain("pevo-promptTokensDisclosure");
    expect(html).toContain("Prompt tokens");
    expect(html).toContain("Developer prompt");
    expect(html).toContain("design");
    expect(html).not.toContain("pevo-contextRing");
    expect(html).not.toContain("pevo-contextBar");
  });
});
