//! `<NAME> runner`（設計 §6・FR5）。契約を stdin で受け、`claude -p` を worktree で回す。
//!
//! 止める条件はただ 1 つ、**rate limit の error record** である。それ以外の失敗は
//! claude の rc をそのまま写す——包みが独自の判定を足すと、呼出側は「誰が止めたか」を
//! 見失う。

use super::{build, feed, fill, flag, need, read_stdin_bytes, Call, DEFAULT_CLAUDE, RC_RATE_LIMIT};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use std::io::{BufRead, BufReader};
use std::path::Path;

/// prompt の文面（tracked な template・絶対 path も口座名も含まない）。
const TEMPLATE: &str = include_str!("runner.txt");

/// 使い方の 1 行。
pub fn usage() -> String {
    format!(
        "usage: {} runner --worktree D --write-set F --plugin-dir D --permission-mode M [--account-dir D] [--claude PATH] < contract",
        crate::name::NAME
    )
}

/// `runner` を 1 回。契約本文は stdin から読む。
pub fn dispatch(args: &[String]) -> Outcome {
    let parsed = (|| {
        Ok::<_, String>((
            need(args, "--worktree")?.to_owned(),
            need(args, "--write-set")?.to_owned(),
            need(args, "--plugin-dir")?.to_owned(),
            need(args, "--permission-mode")?.to_owned(),
            flag(args, "--account-dir")?.map(str::to_owned),
            flag(args, "--claude")?.map(str::to_owned),
        ))
    })();
    let (worktree, write_set, plugin_dir, mode, account, claude) = match parsed {
        Ok(found) => found,
        Err(reason) => return Outcome::failed(RC_REFUSED, vec![format!("runner: {reason}"), usage()]),
    };
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
    })
}

/// claude を回し、rate limit を見たらその場で止める。
fn launch(call: &Call<'_>) -> Outcome {
    let spawned = build(call).spawn();
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

/// stream-json の 1 行が **上限に当たった error record** か。
///
/// **error 系の record の中だけ**で上限の語彙を見る。error でない record は、たとえ上限の
/// 語を含んでいても本文の引用であって事実ではない。上限以外の error は rc 75 にせず
/// claude の rc をそのまま写す——1 つの数に 2 つの意味を載せないためである。
fn is_rate_limit(line: &str) -> bool {
    let body = line.trim_start();
    if !body.starts_with('{') || !is_error_record(body) {
        return false;
    }
    LIMIT_WORDS.iter().any(|word| body.contains(word))
}

/// 前提違反（rc 1 + stderr 1 行・何もしない）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("runner: {reason}"))
}
