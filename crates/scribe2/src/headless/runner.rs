//! `<NAME> runner`（設計 §6・FR5）。契約を stdin で受け、`claude -p` を worktree で回す。
//!
//! 止める条件はただ 1 つ、**rate limit の error record** である。それ以外の失敗は
//! claude の rc をそのまま写す——包みが独自の判定を足すと、呼出側は「誰が止めたか」を
//! 見失う。

use super::{build, feed, fill, flag, need, read_stdin_bytes, Call, DEFAULT_CLAUDE, RC_RATE_LIMIT};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::pipe::declaration::Effective;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// prompt の文面（tracked な template・絶対 path も口座名も含まない）。
const TEMPLATE: &str = include_str!("runner.txt");

/// 使い方の 1 行。
pub fn usage() -> String {
    format!(
        "usage: {} runner --worktree D --write-set F --vessel F --plugin-dir D --permission-mode M [--account-dir D] [--claude PATH] < contract",
        crate::name::NAME
    )
}

/// `runner` を 1 回。契約本文は stdin から読む。
pub fn dispatch(args: &[String]) -> Outcome {
    let parsed = (|| {
        Ok::<_, String>((
            need(args, "--worktree")?.to_owned(),
            need(args, "--write-set")?.to_owned(),
            need(args, "--vessel")?.to_owned(),
            need(args, "--plugin-dir")?.to_owned(),
            need(args, "--permission-mode")?.to_owned(),
            flag(args, "--account-dir")?.map(str::to_owned),
            flag(args, "--claude")?.map(str::to_owned),
        ))
    })();
    let (worktree, write_set, vessel, plugin_dir, mode, account, claude) = match parsed {
        Ok(found) => found,
        Err(reason) => return Outcome::failed(RC_REFUSED, vec![format!("runner: {reason}"), usage()]),
    };
    // **権限は便の写しから読む**（manifest も repo の宣言も読まない＝便の中で権限が
    // 動かない）。写しが読めない周は claude を起こさずに rc 2 で止める。
    let granted = match Effective::load(Path::new(&vessel)) {
        Ok(found) => found,
        Err(errors) => {
            let mut lines = vec!["runner: vessel の写しを読めない".to_owned()];
            lines.extend(errors.iter().map(ToString::to_string));
            return Outcome::failed(RC_BROKEN, lines);
        }
    };
    let tools = allowed_tools(granted.allowed());
    let contract = String::from_utf8_lossy(&read_stdin_bytes()).into_owned();
    if contract.trim().is_empty() {
        return refused("契約が stdin に無い".to_owned());
    }
    let listed = match std::fs::read_to_string(&write_set) {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("runner: write-set を読めない: {err}")),
    };
    // **1 走査で埋める**。重ねて replace すると契約本文の中の `{write_set}` まで展開され、
    // 外から来る text が prompt の構造へ触れられる。
    let prompt = fill(
        TEMPLATE,
        &[("{contract}", contract.trim_end()), ("{write_set}", listed.trim_end())],
    );
    launch(&Call {
        claude: claude.as_deref().unwrap_or(DEFAULT_CLAUDE),
        prompt: &prompt,
        permission_mode: &mode,
        plugin_dir: Some(&plugin_dir),
        account_dir: account.as_deref(),
        cwd: Some(Path::new(&worktree)),
        // rate limit を**途中で**見るので逐次で受ける。
        streaming: true,
    },
    &tools)
}

/// claude を回し、rate limit を見たらその場で止める。
///
/// **権限を与えるのはここだけ**である（lens は判定を受け取るだけ）。settings を 1 つも
/// 読まない起動形（`--setting-sources` の空値 + `--strict-mcp-config`）は runner と lens に
/// 共通なので [`build`] が持ち、この口は**与える権限**だけを足す——器が与えた allow の外は
/// plugin の PermissionRequest hook が deny する（ADR-0011 §2.1 が ADR-0009 §2.1 / ADR-0010
/// §2.4 の起動 flag を部分 supersede・allowlist の形と hook の一律 deny は不変）。
fn launch(call: &Call<'_>, tools: &str) -> Outcome {
    let mut command = build(call);
    command.arg("--allowedTools").arg(tools);
    let spawned = command.spawn();
    let mut child = match spawned {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("runner: claude を起動できない: {err}")),
    };
    feed(&mut child, call.prompt);
    let mut records = 0_usize;
    let mut limited = false;
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            records = records.saturating_add(1);
            if is_rate_limit(&line) {
                limited = true;
                // **待たずに殺す**。上限に当たった席を回し続けても、次の record は
                // 同じ error でしか無い。
                let _ = child.kill();
                break;
            }
        }
    }
    let status = child.wait();
    if limited {
        return Outcome::failed_line(RC_RATE_LIMIT, "runner: rate limit の record を見たので止めた".to_owned());
    }
    match status {
        Err(err) => Outcome::failed_line(RC_BROKEN, format!("runner: claude を待てない: {err}")),
        // **rc は claude のものを写す**（包みが作り替えない）。
        Ok(found) => {
            let rc = u8::try_from(found.code().unwrap_or(i32::from(RC_BROKEN))).unwrap_or(RC_BROKEN);
            let line = format!("runner: rc={rc} records={records}");
            if rc == 0 { Outcome::ok_line(line) } else { Outcome { out: vec![line], err: Vec::new(), rc } }
        }
    }
}

/// 許す command を `--allowedTools` の 1 本へ組む。
///
/// 値は**便ごとに凍結した写し**から来る＝実装が対象 repo の宣言を書き換えても、走っている
/// 便の権限は変わらない（ADR-0010 §2.4）。
fn allowed_tools(commands: &[String]) -> String {
    commands
        .iter()
        .map(|command| format!("Bash({command}:*)"))
        .collect::<Vec<String>>()
        .join(",")
}

/// 上限に当たったことを表す語彙（planner 裁定 2026-09-10 Q4）。
///
/// 1 語に絞ると取りこぼす——claude は上限を `rate_limit` を含まない文言でも surface する。
const LIMIT_WORDS: &[&str] = &[
    "rate_limit",
    "rate limit",
    "usage limit",
    "429",
    "529",
    "overloaded",
];

/// stream-json の 1 行が **error を名乗る record** か。
///
/// 語彙を探す前にここで絞るのが要点である。**本文の引用で誤爆しない**ようにするには、
/// 「どんな行か」を先に構造で決めるしかない（実測 2026-09-10: 応答が契約の文言を引用した
/// だけで rc 75 になっていた——本 bead の契約自身がその文言を含む）。
fn is_error_record(body: &str) -> bool {
    body.contains(r#""is_error":true"#)
        || body.contains(r#""type":"error""#)
        || body.contains(r#""subtype":"error"#)
}

/// 上限語彙を探す **本文 field**（何が起きたかを人が読む形で運ぶ値だけ）。
///
/// **id 類（`session_id` / `uuid` / `request_id`）を入れない**のが本 slice の要点である。
/// 実測 2026-09-10（`s2-07l.64` の実 run）: claude が振った `session_id` の 16 進に `429` が
/// 部分文字列として現れただけで、**401 authentication_failed が rc 75「上限」に化けた**
/// （呼出側は `Failed detail=rate-limit` と記帳する＝便の失敗原因が台帳に嘘で残る）。
const BODY_FIELDS: &[&str] = &["result", "error", "message", "text", "status"];

/// record の中の `field` の値を**字面で**切り出す（JSON parser を足さない）。
///
/// 探すのは `"<field>":` の形だけで、**colon の前の空白（`"result" : …`）は見ない**
/// （claude の stream-json は compact ゆえ実害が無い側へ倒す。取り落とした周は
/// 「上限ではない」＝claude の rc を写す極性に落ちる）。colon の**後ろ**の空白は読む。
///
/// parser を入れれば正確になるが、それは**依存の追加**（A3 の user 承認と C13 の予算）で
/// あり、ここで要るのは「誤爆を塞ぐ」ことであって parser を得ることではない。値は 3 形を
/// 取る——文字列（escape されていない `"` まで）・入れ子（対応する閉じ括弧まで・`{"error":
/// {"type":"rate_limit_error"}}` の形が実在する）・裸の scalar（`,` か `}` まで・`{"type":
/// "error","status":429}` の形が実在する）。同じ field が複数回現れる record もあるので
/// **全ての出現**を返す。
fn field_values<'a>(body: &'a str, field: &str) -> Vec<&'a str> {
    let needle = format!("\"{field}\":");
    let mut values = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find(&needle) {
        let after = rest.get(at.saturating_add(needle.len())..).unwrap_or_default();
        if let Some(found) = value_span(after) {
            values.push(found);
        }
        rest = after;
    }
    values
}

/// `"<field>":` の直後から値 1 つ分の字面を取る。取れなければ `None`。
fn value_span(after: &str) -> Option<&str> {
    let trimmed = after.trim_start();
    match trimmed.chars().next() {
        Some('"') => quoted_span(trimmed),
        Some(open @ ('{' | '[')) => nested_span(trimmed, open),
        Some(_) => {
            let end = trimmed.find([',', '}']).unwrap_or(trimmed.len());
            trimmed.get(..end).map(str::trim)
        }
        None => None,
    }
}

/// 先頭の `"` から **escape されていない** `"` までの中身。
fn quoted_span(trimmed: &str) -> Option<&str> {
    let mut escaped = false;
    for (at, ch) in trimmed.char_indices().skip(1) {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return trimmed.get(1..at);
        }
    }
    None
}

/// 先頭の `{` / `[` から対応する閉じ括弧までの全体（**文字列の中の括弧は数えない**）。
fn nested_span(trimmed: &str, open: char) -> Option<&str> {
    let close = if open == '{' { '}' } else { ']' };
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for (at, ch) in trimmed.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
        } else if ch == open {
            depth = depth.saturating_add(1);
        } else if ch == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return trimmed.get(..at.saturating_add(1));
            }
        }
    }
    None
}

/// stream-json の 1 行が **上限に当たった error record** か。
///
/// 絞りは 2 段である。(1) **error 系の record**だけを見る（error でない record は、たとえ
/// 上限の語を含んでも本文の引用であって事実ではない）。(2) そのうえで語彙を探すのは
/// **本文 field の値の中だけ**で、record 全体の字面には当てない（[`BODY_FIELDS`]）。
///
/// **本文 field の値を 1 つも取れない error record は「上限ではない」**へ倒す。分からない
/// 周を上限と名乗ると原因が失われる（401 が rate-limit として記帳される）が、上限を
/// 取りこぼしても呼出側は claude の rc をそのまま見る＝失う側が小さいほうへ寄せる。
/// 上限以外の error を rc 75 にしないのは従来どおり——1 つの数に 2 つの意味を載せない。
fn is_rate_limit(line: &str) -> bool {
    let body = line.trim_start();
    if !body.starts_with('{') || !is_error_record(body) {
        return false;
    }
    BODY_FIELDS
        .iter()
        .flat_map(|field| field_values(body, field))
        .any(|value| LIMIT_WORDS.iter().any(|word| value.contains(word)))
}

/// 前提違反（rc 1 + stderr 1 行・何もしない）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("runner: {reason}"))
}
