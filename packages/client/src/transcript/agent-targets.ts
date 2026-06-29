import type { TranscriptBlock, TranscriptEntry } from "@psychevo/protocol";

import {
  blocksForEntry,
  isLiveOverlayForTurn,
  recordForValue,
  stringValue
} from "./common";

type AgentChildTarget = {
  agentName: string | null;
  childSessionId: string;
  parentSessionId: string | null;
  task: string | null;
  taskName: string | null;
};

export function enrichCommittedAgentTargetsFromLive(
  entry: TranscriptEntry,
  liveEntries: TranscriptEntry[],
  turnId: string
): TranscriptEntry {
  const liveAgentBlocks = liveEntries
    .filter((candidate) => isLiveOverlayForTurn(candidate, turnId))
    .flatMap(blocksForEntry)
    .filter((block) => block.kind === "agent" && agentChildTargetFromBlock(block));
  if (liveAgentBlocks.length === 0) {
    return entry;
  }
  let changed = false;
  const blocks = blocksForEntry(entry).map((block) => {
    if (block.kind !== "agent" || agentChildTargetFromBlock(block)) {
      return block;
    }
    const liveBlock = liveAgentBlocks.find((candidate) => sameAgentInvocationBlock(block, candidate));
    const target = liveBlock ? agentChildTargetFromBlock(liveBlock) : null;
    if (!target) {
      return block;
    }
    changed = true;
    return blockWithAgentChildTarget(block, target);
  });
  return changed ? { ...entry, blocks } : entry;
}

function sameAgentInvocationBlock(committed: TranscriptBlock, live: TranscriptBlock): boolean {
  if (committed.id === live.id) {
    return true;
  }
  const committedToolCallId = agentBlockToolCallId(committed);
  const liveToolCallId = agentBlockToolCallId(live);
  return Boolean(committedToolCallId && liveToolCallId && committedToolCallId === liveToolCallId);
}

function agentBlockToolCallId(block: TranscriptBlock): string | null {
  const metadata = recordForValue(block.metadata);
  const resultMetadata = recordForValue(block.result?.metadata);
  return stringValue(metadata.tool_call_id)
    ?? stringValue(metadata.toolCallId)
    ?? stringValue(resultMetadata.tool_call_id)
    ?? stringValue(resultMetadata.toolCallId);
}

function agentChildTargetFromBlock(block: TranscriptBlock): AgentChildTarget | null {
  const metadata = recordForValue(block.metadata);
  const metadataResult = recordForValue(metadata.result);
  const resultMetadata = recordForValue(block.result?.metadata);
  const resultMetadataResult = recordForValue(resultMetadata.result);
  const blockResultContent = jsonRecord(block.result?.content);
  const blockBody = jsonRecord(block.body);
  const records = [
    metadata,
    metadataResult,
    resultMetadata,
    resultMetadataResult,
    blockResultContent,
    blockBody
  ];
  const childSessionId = firstStringField(records, [
    "child_thread_id",
    "childThreadId",
    "child_session_id",
    "childSessionId",
    "session_id",
    "sessionId"
  ]);
  if (!childSessionId) {
    return null;
  }
  return {
    agentName: firstStringField(records, ["agent_name", "agentName", "name"]),
    childSessionId,
    parentSessionId: firstStringField(records, ["parent_thread_id", "parentThreadId", "parent_session_id", "parentSessionId"]),
    task: firstStringField(records, ["message", "task", "prompt"]),
    taskName: firstStringField(records, ["task_name", "taskName"])
  };
}

function blockWithAgentChildTarget(block: TranscriptBlock, target: AgentChildTarget): TranscriptBlock {
  const metadata = { ...recordForValue(block.metadata) };
  addAgentTargetFields(metadata, target);
  const metadataResult = { ...recordForValue(metadata.result) };
  addAgentTargetFields(metadataResult, target);
  metadata.result = metadataResult;
  const result = block.result
    ? {
        ...block.result,
        metadata: resultMetadataWithAgentTarget(block.result.metadata, target)
      }
    : block.result;
  return {
    ...block,
    metadata,
    result
  };
}

function resultMetadataWithAgentTarget(metadata: unknown, target: AgentChildTarget): unknown {
  const record = { ...recordForValue(metadata) };
  const result = { ...recordForValue(record.result) };
  addAgentTargetFields(record, target);
  addAgentTargetFields(result, target);
  record.result = result;
  return record;
}

function addAgentTargetFields(record: Record<string, unknown>, target: AgentChildTarget) {
  setStringIfMissing(record, "child_session_id", target.childSessionId);
  setStringIfMissing(record, "child_thread_id", target.childSessionId);
  setStringIfMissing(record, "session_id", target.childSessionId);
  setStringIfMissing(record, "parent_session_id", target.parentSessionId);
  setStringIfMissing(record, "parent_thread_id", target.parentSessionId);
  setStringIfMissing(record, "agent_name", target.agentName);
  setStringIfMissing(record, "task_name", target.taskName);
  setStringIfMissing(record, "task", target.task);
}

function setStringIfMissing(record: Record<string, unknown>, key: string, value: string | null) {
  if (value && !stringValue(record[key])) {
    record[key] = value;
  }
}

function firstStringField(records: Array<Record<string, unknown>>, keys: string[]): string | null {
  for (const record of records) {
    for (const key of keys) {
      const value = stringValue(record[key]);
      if (value) {
        return value;
      }
    }
  }
  return null;
}

function jsonRecord(value: unknown): Record<string, unknown> {
  if (typeof value !== "string") {
    return {};
  }
  try {
    return recordForValue(JSON.parse(value));
  } catch {
    return {};
  }
}
