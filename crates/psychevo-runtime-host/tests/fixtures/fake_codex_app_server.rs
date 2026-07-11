use std::env;
use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::time::Duration;

fn main() {
    let scenario = env::var("CODEX_FAKE_SCENARIO").unwrap_or_else(|_| "ordering".to_string());
    let log_path = env::var("CODEX_FAKE_LOG").ok();
    let session_cwd = env::var("CODEX_FAKE_CWD").unwrap_or_else(|_| {
        env::current_dir()
            .expect("fake Codex cwd")
            .to_string_lossy()
            .to_string()
    });
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut turn_count = 0usize;
    let mut interaction_responses = 0usize;

    for line in stdin.lock().lines() {
        let line = line.expect("stdin line");
        if let Some(path) = &log_path {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("fake log");
            writeln!(file, "{line}").expect("write fake log");
        }

        if line.contains("\"id\":\"approval-")
            || line.contains("\"id\":\"file-")
            || line.contains("\"id\":\"permission-")
            || line.contains("\"id\":\"question-")
        {
            interaction_responses += 1;
            if scenario == "interactions" && interaction_responses == 4 {
                emit_agent_and_terminal(&mut stdout, "native-1", "turn-native-1", "approved");
            } else if scenario == "child_interaction" && interaction_responses == 1 {
                emit_agent_and_terminal(&mut stdout, "native-1", "turn-native-2", "child approved");
            }
            continue;
        }

        let Some(method) = string_field(&line, "method") else {
            continue;
        };
        let id = id_token(&line);
        match method.as_str() {
            "initialize" => respond(
                &mut stdout,
                id.as_deref().expect("initialize id"),
                if scenario == "legacy_version" {
                    r#"{"userAgent":"codex_cli_rs/0.142.5","codexHome":"/tmp/codex","platformFamily":"unix","platformOs":"linux"}"#
                } else {
                    r#"{"userAgent":"codex_cli_rs/0.143.0-fixture","codexHome":"/tmp/codex","platformFamily":"unix","platformOs":"linux"}"#
                },
            ),
            "initialized" => {
                if scenario == "ordering" {
                    for index in 0..200 {
                        eprintln!("diagnostic-{index}");
                    }
                }
            }
            "thread/start" => {
                if scenario == "ordering" {
                    notify(
                        &mut stdout,
                        "thread/started",
                        r#"{"thread":{"id":"native-1","turns":[]}}"#,
                    );
                }
                respond(
                    &mut stdout,
                    id.as_deref().expect("thread/start id"),
                    r#"{"thread":{"id":"native-1","modelProvider":"openai","turns":[]},"model":"gpt-fixture","modelProvider":"openai"}"#,
                );
            }
            "thread/resume" => respond(
                &mut stdout,
                id.as_deref().expect("thread/resume id"),
                if line.contains("\"threadId\":\"child-1\"") {
                    r#"{"thread":{"id":"child-1","parentThreadId":"native-1","modelProvider":"openai","turns":[]},"model":"gpt-fixture","modelProvider":"openai"}"#
                } else {
                    r#"{"thread":{"id":"native-1","modelProvider":"openai","turns":[]},"model":"gpt-fixture","modelProvider":"openai"}"#
                },
            ),
            "thread/compact/start" => {
                respond(
                    &mut stdout,
                    id.as_deref().expect("thread/compact/start id"),
                    "{}",
                );
                if scenario == "compact_eof" {
                    return;
                }
                assert_eq!(scenario, "compact", "unexpected compaction scenario");
                std::thread::sleep(Duration::from_millis(150));
                notify(
                    &mut stdout,
                    "item/started",
                    r#"{"threadId":"native-1","turnId":"compact-turn-1","item":{"type":"contextCompaction","id":"native-compaction-1"},"startedAtMs":1}"#,
                );
                notify(
                    &mut stdout,
                    "item/completed",
                    r#"{"threadId":"native-1","turnId":"compact-turn-1","item":{"type":"contextCompaction","id":"native-compaction-1"},"completedAtMs":2}"#,
                );
                terminal(&mut stdout, "native-1", "compact-turn-1", "completed");
            }
            "thread/goal/get" => respond(
                &mut stdout,
                id.as_deref().expect("thread/goal/get id"),
                r#"{"goal":{"threadId":"native-1","objective":"Ship auxiliary state","status":"active","tokenBudget":1000,"tokensUsed":120,"timeUsedSeconds":9,"createdAt":100,"updatedAt":110}}"#,
            ),
            "thread/goal/set" => respond(
                &mut stdout,
                id.as_deref().expect("thread/goal/set id"),
                r#"{"goal":{"threadId":"native-1","objective":"Ship auxiliary state","status":"active","tokenBudget":1000,"tokensUsed":120,"timeUsedSeconds":9,"createdAt":100,"updatedAt":110}}"#,
            ),
            "thread/goal/clear" => respond(
                &mut stdout,
                id.as_deref().expect("thread/goal/clear id"),
                r#"{"cleared":true}"#,
            ),
            "account/rateLimits/read" => respond(
                &mut stdout,
                id.as_deref().expect("account/rateLimits/read id"),
                r#"{"rateLimits":{"limitId":"codex","limitName":"Codex","primary":{"usedPercent":10,"windowDurationMins":300,"resetsAt":999},"secondary":{"usedPercent":20,"windowDurationMins":10080,"resetsAt":1999},"credits":{"hasCredits":true,"unlimited":false,"balance":"12.50"},"individualLimit":{"limit":"100","used":"20","remainingPercent":80,"resetsAt":2999},"planType":"pro","rateLimitReachedType":"rate_limit_reached"},"rateLimitsByLimitId":{"codex":{"limitId":"codex","limitName":"Codex","primary":{"usedPercent":10,"windowDurationMins":300,"resetsAt":999},"secondary":{"usedPercent":20,"windowDurationMins":10080,"resetsAt":1999},"credits":{"hasCredits":true,"unlimited":false,"balance":"12.50"},"individualLimit":{"limit":"100","used":"20","remainingPercent":80,"resetsAt":2999},"planType":"pro","rateLimitReachedType":"rate_limit_reached"}},"rateLimitResetCredits":{"availableCount":2,"credits":[{"id":"credit-1","resetType":"codexRateLimits","status":"available","grantedAt":90,"expiresAt":190,"title":"Reset","description":"Fixture reset"}]}}"#,
            ),
            "model/list" => respond(
                &mut stdout,
                id.as_deref().expect("model/list id"),
                r#"{"data":[{"id":"gpt-fixture","model":"gpt-fixture","upgrade":null,"upgradeInfo":null,"availabilityNux":null,"displayName":"GPT Fixture","description":"Deterministic fake model","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"medium","description":"Balanced reasoning"},{"reasoningEffort":"high","description":"Deeper reasoning"}],"defaultReasoningEffort":"medium","inputModalities":["text"],"supportsPersonality":true,"additionalSpeedTiers":[],"serviceTiers":[{"id":"fast","name":"Fast","description":"Priority processing"}],"defaultServiceTier":null,"isDefault":true},{"id":"gpt-fixture-mini","model":"gpt-fixture-mini","upgrade":null,"upgradeInfo":null,"availabilityNux":null,"displayName":"GPT Fixture Mini","description":"Second deterministic fake model","hidden":false,"supportedReasoningEfforts":[{"reasoningEffort":"low","description":"Fast reasoning"},{"reasoningEffort":"medium","description":"Balanced reasoning"}],"defaultReasoningEffort":"low","inputModalities":["text"],"supportsPersonality":false,"additionalSpeedTiers":[],"serviceTiers":[{"id":"flex","name":"Flex","description":"Flexible processing"}],"defaultServiceTier":null,"isDefault":false}],"nextCursor":null}"#,
            ),
            "turn/start" => {
                turn_count += 1;
                let native_turn_id = format!("turn-native-{turn_count}");
                match scenario.as_str() {
                    "ordering" | "legacy_version" | "steer"
                        if scenario != "steer" || turn_count == 1 =>
                    {
                        notify(
                            &mut stdout,
                            "turn/started",
                            &format!(
                                r#"{{"threadId":"native-1","turn":{{"id":"{native_turn_id}","items":[],"status":"inProgress"}}}}"#
                            ),
                        );
                        notify(
                            &mut stdout,
                            "item/agentMessage/delta",
                            &format!(
                                r#"{{"threadId":"native-1","turnId":"{native_turn_id}","itemId":"message-1","delta":"hel"}}"#
                            ),
                        );
                        emit_agent_and_terminal(&mut stdout, "native-1", &native_turn_id, "hello");
                        terminal(&mut stdout, "native-1", &native_turn_id, "completed");
                        respond_turn(
                            &mut stdout,
                            id.as_deref().expect("turn/start id"),
                            &native_turn_id,
                        );
                    }
                    "steer" => {
                        notify(
                            &mut stdout,
                            "turn/started",
                            &format!(
                                r#"{{"threadId":"native-1","turn":{{"id":"{native_turn_id}","items":[],"status":"inProgress"}}}}"#
                            ),
                        );
                        respond_turn(
                            &mut stdout,
                            id.as_deref().expect("turn/start id"),
                            &native_turn_id,
                        );
                    }
                    "eof" => {
                        respond_turn(
                            &mut stdout,
                            id.as_deref().expect("turn/start id"),
                            &native_turn_id,
                        );
                        return;
                    }
                    "no_retry" => error(
                        &mut stdout,
                        id.as_deref().expect("turn/start id"),
                        -32000,
                        "rejected",
                    ),
                    "interactions" => {
                        respond_turn(
                            &mut stdout,
                            id.as_deref().expect("turn/start id"),
                            &native_turn_id,
                        );
                        request(
                            &mut stdout,
                            "approval-1",
                            "item/commandExecution/requestApproval",
                            r#"{"threadId":"native-1","turnId":"turn-native-1","itemId":"cmd-1","startedAtMs":1,"command":"cargo test","cwd":"/tmp","reason":"Run tests?"}"#,
                        );
                        request(
                            &mut stdout,
                            "file-1",
                            "item/fileChange/requestApproval",
                            r#"{"threadId":"native-1","turnId":"turn-native-1","itemId":"file-1","startedAtMs":1,"reason":"Apply patch?","grantRoot":null}"#,
                        );
                        request(
                            &mut stdout,
                            "permission-1",
                            "item/permissions/requestApproval",
                            r#"{"threadId":"native-1","turnId":"turn-native-1","itemId":"permission-1","environmentId":null,"startedAtMs":1,"cwd":"/tmp","reason":"Allow network?","permissions":{"network":{"enabled":true},"fileSystem":null}}"#,
                        );
                        request(
                            &mut stdout,
                            "question-1",
                            "item/tool/requestUserInput",
                            r#"{"threadId":"native-1","turnId":"turn-native-1","itemId":"question-item","questions":[{"id":"confirm","header":"Confirm","question":"Continue?","isOther":true,"options":[{"label":"Yes","description":"Continue"},{"label":"No","description":"Stop"}]},{"id":"details","header":"Details","question":"Anything else?","isSecret":true,"options":null}]}"#,
                        );
                    }
                    "stale" if turn_count == 1 => {
                        respond_turn(
                            &mut stdout,
                            id.as_deref().expect("turn/start id"),
                            &native_turn_id,
                        );
                        emit_agent_and_terminal(&mut stdout, "native-1", &native_turn_id, "first");
                    }
                    "stale" => {
                        notify(
                            &mut stdout,
                            "item/agentMessage/delta",
                            r#"{"threadId":"native-1","turnId":"turn-native-1","itemId":"old","delta":"STALE"}"#,
                        );
                        notify(
                            &mut stdout,
                            "item/agentMessage/delta",
                            &format!(
                                r#"{{"threadId":"native-1","turnId":"{native_turn_id}","itemId":"new","delta":"sec"}}"#
                            ),
                        );
                        emit_agent_and_terminal(&mut stdout, "native-1", &native_turn_id, "second");
                        respond_turn(
                            &mut stdout,
                            id.as_deref().expect("turn/start id"),
                            &native_turn_id,
                        );
                    }
                    "child" => {
                        respond_turn(
                            &mut stdout,
                            id.as_deref().expect("turn/start id"),
                            &native_turn_id,
                        );
                        notify(
                            &mut stdout,
                            "item/completed",
                            &format!(
                                r#"{{"threadId":"native-1","turnId":"{native_turn_id}","item":{{"type":"subAgentActivity","id":"spawn-1","kind":"started","agentThreadId":"child-1","agentPath":"worker"}}}}"#
                            ),
                        );
                        emit_agent_and_terminal(
                            &mut stdout,
                            "native-1",
                            &native_turn_id,
                            "parent done",
                        );
                    }
                    "child_interaction" if turn_count == 1 => {
                        respond_turn(
                            &mut stdout,
                            id.as_deref().expect("turn/start id"),
                            &native_turn_id,
                        );
                        notify(
                            &mut stdout,
                            "item/completed",
                            &format!(
                                r#"{{"threadId":"native-1","turnId":"{native_turn_id}","item":{{"type":"subAgentActivity","id":"spawn-child-interaction","kind":"started","agentThreadId":"child-1","agentPath":"reviewer"}}}}"#
                            ),
                        );
                        emit_agent_and_terminal(
                            &mut stdout,
                            "native-1",
                            &native_turn_id,
                            "child ready",
                        );
                    }
                    "child_interaction" => {
                        respond_turn(
                            &mut stdout,
                            id.as_deref().expect("turn/start id"),
                            &native_turn_id,
                        );
                        notify(
                            &mut stdout,
                            "turn/started",
                            &format!(
                                r#"{{"threadId":"native-1","turn":{{"id":"{native_turn_id}","items":[],"status":"inProgress"}}}}"#
                            ),
                        );
                        // A real child asks only after the client has correlated the
                        // turn/start response. Keep that protocol boundary explicit
                        // instead of emitting the server request in the same read burst.
                        std::thread::sleep(Duration::from_millis(25));
                        request(
                            &mut stdout,
                            "approval-child-1",
                            "item/commandExecution/requestApproval",
                            r#"{"threadId":"child-1","turnId":"turn-child-1","itemId":"child-command-1","startedAtMs":1,"command":"cargo test","cwd":"/tmp","reason":"Allow the child reviewer to run the deterministic checks?"}"#,
                        );
                    }
                    "abort" => {
                        respond_turn(
                            &mut stdout,
                            id.as_deref().expect("turn/start id"),
                            &native_turn_id,
                        );
                    }
                    "auxiliary" => {
                        notify(
                            &mut stdout,
                            "turn/plan/updated",
                            &format!(
                                r#"{{"threadId":"native-1","turnId":"{native_turn_id}","explanation":"Check then ship","plan":[{{"step":"Inspect runtime","status":"completed"}},{{"step":"Ship adapter","status":"inProgress"}}]}}"#
                            ),
                        );
                        notify(
                            &mut stdout,
                            "turn/diff/updated",
                            &format!(
                                r#"{{"threadId":"native-1","turnId":"{native_turn_id}","diff":"diff --git a/a.rs b/a.rs\n+typed"}}"#
                            ),
                        );
                        notify(
                            &mut stdout,
                            "thread/tokenUsage/updated",
                            &format!(
                                r#"{{"threadId":"native-1","turnId":"{native_turn_id}","tokenUsage":{{"total":{{"totalTokens":120,"inputTokens":80,"cachedInputTokens":20,"outputTokens":40,"reasoningOutputTokens":10}},"last":{{"totalTokens":30,"inputTokens":20,"cachedInputTokens":5,"outputTokens":10,"reasoningOutputTokens":3}},"modelContextWindow":200000}}}}"#
                            ),
                        );
                        notify(
                            &mut stdout,
                            "thread/goal/updated",
                            &format!(
                                r#"{{"threadId":"native-1","turnId":"{native_turn_id}","goal":{{"threadId":"native-1","objective":"Ship auxiliary state","status":"active","tokenBudget":1000,"tokensUsed":150,"timeUsedSeconds":12,"createdAt":100,"updatedAt":120}}}}"#
                            ),
                        );
                        notify(
                            &mut stdout,
                            "account/rateLimits/updated",
                            r#"{"rateLimits":{"limitId":"codex","primary":{"usedPercent":42,"windowDurationMins":300,"resetsAt":1099}}}"#,
                        );
                        emit_agent_and_terminal(
                            &mut stdout,
                            "native-1",
                            &native_turn_id,
                            "auxiliary done",
                        );
                        respond_turn(
                            &mut stdout,
                            id.as_deref().expect("turn/start id"),
                            &native_turn_id,
                        );
                    }
                    _ => unreachable!("unknown scenario"),
                }
            }
            "turn/interrupt" => {
                respond(&mut stdout, id.as_deref().expect("interrupt id"), "{}");
                terminal(&mut stdout, "native-1", "turn-native-1", "interrupted");
            }
            "turn/steer" => {
                respond(
                    &mut stdout,
                    id.as_deref().expect("steer id"),
                    r#"{"turnId":"turn-native-2"}"#,
                );
                if scenario == "steer" {
                    emit_agent_and_terminal(
                        &mut stdout,
                        "native-1",
                        "turn-native-2",
                        "steered through public control",
                    );
                }
            }
            "account/read" => respond(
                &mut stdout,
                id.as_deref().expect("account/read id"),
                r#"{"account":null,"requiresOpenaiAuth":true}"#,
            ),
            "account/login/start" => {
                let result = if line.contains("\"type\":\"chatgptDeviceCode\"") {
                    r#"{"type":"chatgptDeviceCode","loginId":"login-device-fixture","verificationUrl":"https://auth.example/device","userCode":"ABCD-1234"}"#
                } else {
                    r#"{"type":"chatgpt","loginId":"login-fixture","authUrl":"https://chatgpt.example/login"}"#
                };
                respond(
                    &mut stdout,
                    id.as_deref().expect("account/login/start id"),
                    result,
                );
            }
            "account/login/cancel" => respond(
                &mut stdout,
                id.as_deref().expect("account/login/cancel id"),
                r#"{"status":"canceled"}"#,
            ),
            "account/logout" => {
                respond(&mut stdout, id.as_deref().expect("account/logout id"), "{}")
            }
            "thread/list" => {
                let data = if line.contains("\"archived\":true") {
                    format!(
                        r#"{{"data":[{{"id":"archived-1","parentThreadId":null,"preview":"Archived","cwd":{},"updatedAt":9,"status":{{"type":"idle"}},"turns":[]}}],"nextCursor":null}}"#,
                        json_string(&session_cwd),
                    )
                } else {
                    format!(
                        r#"{{"data":[{{"id":"root-1","parentThreadId":null,"preview":"Root","cwd":{},"updatedAt":10,"status":{{"type":"idle"}},"turns":[]}},{{"id":"child-1","parentThreadId":"root-1","preview":"Child","cwd":{},"updatedAt":11,"status":{{"type":"idle"}},"turns":[]}}],"nextCursor":null}}"#,
                        json_string(&session_cwd),
                        json_string(&session_cwd),
                    )
                };
                respond(&mut stdout, id.as_deref().expect("thread/list id"), &data);
            }
            "thread/read" => respond(
                &mut stdout,
                id.as_deref().expect("thread/read id"),
                &format!(
                    r#"{{"thread":{{"id":"child-1","parentThreadId":"root-1","preview":"Child","cwd":{},"updatedAt":11,"status":{{"type":"idle"}},"turns":[{{"id":"history-turn","itemsView":"full","status":"completed","startedAt":12,"items":[{{"type":"userMessage","id":"user-1","content":[{{"type":"text","text":"hello"}}]}},{{"type":"agentMessage","id":"assistant-1","text":"hi"}}]}}]}}}}"#,
                    json_string(&session_cwd),
                ),
            ),
            "thread/fork" => respond(
                &mut stdout,
                id.as_deref().expect("thread/fork id"),
                r#"{"thread":{"id":"fork-1","parentThreadId":null,"preview":"Fork","cwd":"/tmp","turns":[]}}"#,
            ),
            "thread/name/set" | "thread/archive" | "thread/delete" => {
                respond(&mut stdout, id.as_deref().expect("mutation id"), "{}")
            }
            "thread/unarchive" => respond(
                &mut stdout,
                id.as_deref().expect("unarchive id"),
                r#"{"thread":{"id":"root-1","parentThreadId":null,"preview":"Root","cwd":"/tmp","turns":[]}}"#,
            ),
            _ => error(
                &mut stdout,
                id.as_deref().unwrap_or("0"),
                -32601,
                &format!("unknown method: {method}"),
            ),
        }
    }
}

fn string_field(line: &str, field: &str) -> Option<String> {
    let marker = format!("\"{field}\":\"");
    let rest = line.split_once(&marker)?.1;
    Some(rest.split('"').next()?.to_string())
}

fn json_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

fn id_token(line: &str) -> Option<String> {
    let rest = line.split_once("\"id\":")?.1.trim_start();
    if let Some(rest) = rest.strip_prefix('"') {
        let value = rest.split('"').next()?;
        Some(format!("\"{value}\""))
    } else {
        Some(
            rest.split([',', '}'])
                .next()
                .expect("numeric id")
                .to_string(),
        )
    }
}

fn line(stdout: &mut impl Write, value: &str) {
    writeln!(stdout, "{value}").expect("write stdout");
    stdout.flush().expect("flush stdout");
}

fn respond(stdout: &mut impl Write, id: &str, result: &str) {
    line(stdout, &format!(r#"{{"id":{id},"result":{result}}}"#));
}

fn error(stdout: &mut impl Write, id: &str, code: i64, message: &str) {
    line(
        stdout,
        &format!(r#"{{"id":{id},"error":{{"code":{code},"message":"{message}"}}}}"#),
    );
}

fn notify(stdout: &mut impl Write, method: &str, params: &str) {
    line(
        stdout,
        &format!(r#"{{"method":"{method}","params":{params}}}"#),
    );
}

fn request(stdout: &mut impl Write, id: &str, method: &str, params: &str) {
    line(
        stdout,
        &format!(r#"{{"id":"{id}","method":"{method}","params":{params}}}"#),
    );
}

fn respond_turn(stdout: &mut impl Write, id: &str, turn_id: &str) {
    respond(
        stdout,
        id,
        &format!(r#"{{"turn":{{"id":"{turn_id}","items":[],"status":"inProgress"}}}}"#),
    );
}

fn emit_agent_and_terminal(stdout: &mut impl Write, thread_id: &str, turn_id: &str, text: &str) {
    notify(
        stdout,
        "item/completed",
        &format!(
            r#"{{"threadId":"{thread_id}","turnId":"{turn_id}","item":{{"type":"agentMessage","id":"message-final","text":"{text}"}}}}"#
        ),
    );
    terminal(stdout, thread_id, turn_id, "completed");
}

fn terminal(stdout: &mut impl Write, thread_id: &str, turn_id: &str, status: &str) {
    notify(
        stdout,
        "turn/completed",
        &format!(
            r#"{{"threadId":"{thread_id}","turn":{{"id":"{turn_id}","items":[],"status":"{status}","error":null}}}}"#
        ),
    );
}
