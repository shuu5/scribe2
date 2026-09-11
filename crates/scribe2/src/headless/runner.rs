//! `<NAME> runner`（設計 §6・FR5）。契約を stdin で受け、`claude -p` を worktree で回す。
//!
//! 止める条件はただ 1 つ、**上限 record（`rate_limit_event`）の status が「止める側」の集合に
//! 属すること**である（ADR-0012 §2.1）。それ以外の失敗は claude の rc をそのまま写す——包みが
//! 独自の判定を足すと、呼出側は「誰が止めたか」を見失う。

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
    let prompt = compose(&contract, &listed, granted.allowed());
    // **claude を起こす前に**残す（起きた後に書く形だと、席が止まらない周の prompt が読めない）。
    let unsaved = save_prompt(Path::new(&vessel), &prompt).err();
    let mut outcome = launch(&Call {
        claude: claude.as_deref().unwrap_or(DEFAULT_CLAUDE),
        prompt: &prompt,
        permission_mode: &mode,
        plugin_dir: Some(&plugin_dir),
        account_dir: account.as_deref(),
        cwd: Some(Path::new(&worktree)),
        // rate limit を**途中で**見るので逐次で受ける。
        streaming: true,
    },
    &tools);
    if let Some(reason) = unsaved {
        outcome.err.push(format!("runner: prompt を残せない: {reason}"));
    }
    outcome
}

/// prompt を組む（template の 3 対を **1 走査**で埋める）。
///
/// 重ねて replace すると、先に埋めた契約本文や write-set の中の `{allowed}` / `{write_set}` が
/// 次の走査で展開され、外から来る text が prompt の構造へ触れられる。`{allowed}` は人が読む面の
/// allowlist（`--allowedTools` の `Bash(<cmd>:*)` 形は [`allowed_tools`] が別に組む）で、値の出所は
/// **便の写し**のまま＝manifest も対象 repo の宣言も読まない（ADR-0010 §2.4）。
fn compose(contract: &str, write_set: &str, allowed: &[String]) -> String {
    let listed_allowed = allowed
        .iter()
        .map(|command| format!("- {command}"))
        .collect::<Vec<String>>()
        .join("\n");
    fill(
        TEMPLATE,
        &[
            ("{contract}", contract.trim_end()),
            ("{write_set}", write_set.trim_end()),
            ("{allowed}", &listed_allowed),
        ],
    )
}

/// 残す prompt の file 名。置き場は **vessel の写しの隣**（= run dir・`<state_dir>/pipe/<run>/`）。
const PROMPT_FILE: &str = "prompt.txt";

/// runner が組んだ prompt を便の作業面へ残す（設計 §6・`s2-07l.79`）。
///
/// prompt は stdin で渡すので stream には 1 行も出ない。「何を渡したか」を後から読める口が
/// これで、置き場は写しの隣＝便ごとの run dir である（新しい flag も env も足さない・C2.2）。
/// **tracked な面〔worktree〕には置かない**（契約本文と write-set を PUBLIC 面へ出さない）——
/// 置き場が解けない写しでは cwd へ落とす代わりに残さない側へ倒す。
///
/// **判定の入力ではない**——残せない周も便は続き、rc は claude のものを写す。呼び手は
/// `Err` を stderr の 1 行にするだけで止めない（証跡の欠落で claude を起こさない形にしない）。
fn save_prompt(vessel: &Path, prompt: &str) -> Result<(), String> {
    // **置き場が解けない写し（`vessel.toml` のような裸の名）は残さない**。`Path::parent` は
    // 裸の名に `Some("")` を返すので、そのまま join すると prompt が runner の **cwd**（＝
    // pipeline では便の worktree・tracked 面）へ落ちる（lens 2026-09-11 M2 の実測）。
    let dir = vessel
        .parent()
        .filter(|found| !found.as_os_str().is_empty())
        .ok_or_else(|| format!("{} の置き場を解けない（写しは dir 付きの path で渡す）", vessel.display()))?;
    let path = dir.join(PROMPT_FILE);
    std::fs::write(&path, prompt).map_err(|err| format!("{} を書けない: {err}", path.display()))
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
    // **観測した status は記録に残す**（ADR-0012 §2.1 末尾）。集合を**実測で育てる**唯一の口で、
    // これが無いと集合は永久に空のままになる。**判定の入力ではない**ので、残せない周も便は続く。
    let mut seen_status: Option<String> = None;
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            records = records.saturating_add(1);
            match decide(&line, STOP_STATUSES) {
                Decision::Ignore => continue,
                Decision::Observed(status) => seen_status = Some(status),
                Decision::Stop(status) => {
                    seen_status = Some(status);
                    limited = true;
                    // **待たずに殺す**。上限に当たった席を回し続けても、次の record は
                    // 同じ上限でしか無い。
                    let _ = child.kill();
                    break;
                }
            }
        }
    }
    let status = child.wait();
    let observed = observed_suffix(seen_status.as_deref());
    if limited {
        let reason = seen_status.as_deref().unwrap_or_default();
        return Outcome::failed_line(RC_RATE_LIMIT, stop_line(reason));
    }
    match status {
        Err(err) => Outcome::failed_line(RC_BROKEN, format!("runner: claude を待てない: {err}")),
        // **rc は claude のものを写す**（包みが作り替えない）。
        Ok(found) => {
            let rc = u8::try_from(found.code().unwrap_or(i32::from(RC_BROKEN))).unwrap_or(RC_BROKEN);
            let line = format!("runner: rc={rc} records={records}{observed}");
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

/// **上限を表す status の閉じた集合**（[ADR-0012] §2.1）。
///
/// **実測で採れた値だけ**を入れる（MUST）。記録時点で採れている status は `allowed_warning`
/// （＝「許可されつつ警告」の周で、止める側ではない）1 つだけなので、**この集合は空**である。
/// ゆえに器は当面 rc 75 を一度も立てない——これは ADR の**決定**であって実装の手抜きではない。
///
/// 逆（許可側を列挙して未知を上限へ倒す）を採らない理由は**失敗の重さの非対称**である:
/// rc 75 は判定の名札ではなく**実行の中断**（下の `launch` は待たずに kill する）。取りこぼした
/// 周は「rc 75 という分類が付かない」だけで便は claude 自身の rc で止まるが、誤って上限と読んだ
/// 周は**健全な便が殺される**（ADR-0012 §4 案 (A') の却下理由）。
///
/// 止まる周の status を採取したら、その値をここへ入れる。**推測で足さない**（MUST NOT）。
///
/// [ADR-0012]: ../../../design-intent/decisions/ADR-0012-rate-limit-detection-reads-dedicated-record.html
const STOP_STATUSES: &[&str] = &[];

/// 上限 record の種別（`type` field の値）。
const EVENT_KIND: &str = "rate_limit_event";

/// 構造化された上限情報の key。
const INFO_KEY: &str = "rate_limit_info";

/// 判定の入力になる field の key。
const STATUS_KEY: &str = "status";

/// status が**止める側**か（純関数）。
///
/// 集合が空である以上、production の経路は**止まる側を一度も通らない**。判定をここへ切り出す
/// のは、そうしないと**上限で止まる分岐に歯が 1 本も当たらない**からである（歯は非空の集合を
/// 渡して両向きに測る）。
pub fn stops_on(status: &str, stop_statuses: &[&str]) -> bool {
    stop_statuses.contains(&status)
}

/// 1 行に対する判定（純関数の返り値）。
///
/// **3 値**にするのは、`launch` の loop を薄くして**判定そのものを歯で測れるようにする**ため
/// である。集合が空である以上 production は [`Decision::Stop`] を通らないので、分岐が loop の中に
/// 埋まっていると**上限で止まる側に歯が 1 本も当たらない**（lens 2026-09-11 H1）。
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// 上限 record ではない（何もしない）。
    Ignore,
    /// 上限 record だが**止める側ではない** status（記録だけする）。
    Observed(String),
    /// 止める側の status（rc 75 で中断する）。
    Stop(String),
}

/// 1 行を判定する（純関数）。
pub fn decide(line: &str, stop_statuses: &[&str]) -> Decision {
    let Some(status) = rate_limit_status(line) else {
        return Decision::Ignore;
    };
    if stops_on(status, stop_statuses) {
        return Decision::Stop(status.to_owned());
    }
    Decision::Observed(status.to_owned())
}

/// 上限で止めた周の 1 行（記録面と同じ形で status を載せる）。
pub fn stop_line(status: &str) -> String {
    format!("runner: rate limit の record を見たので止めた rate-limit-status={status}")
}

/// 観測を記録面へ載せる後置き（観測していない周は空）。
pub fn observed_suffix(status: Option<&str>) -> String {
    status
        .map(|value| format!(" rate-limit-status={value}"))
        .unwrap_or_default()
}

/// stream-json の 1 行から **`rate_limit_event` の `rate_limit_info.status`** を読む。
///
/// 上限の真の合図は**専用の record 種別と構造化された status** で来る（実 run の raw stream から
/// 採取・ADR-0012 §1）。本文の語彙を探す形はこの便で撤去した——探していたのは合図ではなく
/// 「同じ言葉が混じった別の文」で、識別子の 16 進や tool の出力で 2 度誤爆した。
///
/// 読むのは **`rate_limit_info` の直下の `status`** だけである（入れ子の object に入ったら
/// そこで打ち切る）。`rate_limit_info` は `unifiedWindows` のような入れ子を持つので、最初に
/// 見つけた `"status"` を採る形だと **key の並び次第で別の object の値を読む**（lens 2026-09-11 H3）。
///
/// key と colon の間・colon と値の間の**空白に寛容**である。実 stream は compact だが（実測）、
/// 表記が変わっただけで記録の口が無音で止まる形にはしない（lens 2026-09-11 H2）。
///
/// **限界**: record 種別は行の中の marker で見るので、**別の record が上限 record を入れ子で
/// 引用した周**は status を返しうる（top-level の種別を読む実装は ADR-0012 §2.2 が撤去を命じた）。
/// 集合が空である現在は止まらないが、記録の口には載る＝集合を育てるときは現物を確かめる。
pub fn rate_limit_status(line: &str) -> Option<&str> {
    let body = line.trim_start();
    if !body.starts_with('{') || !has_pair(body, "type", EVENT_KIND) {
        return None;
    }
    let at = find_key(body, INFO_KEY)?;
    let rest = body.get(at..)?;
    let inner = immediate_object(rest)?;
    let status_at = find_key(inner, STATUS_KEY)?;
    let value = inner.get(status_at..)?;
    quoted_value(value)
}

/// `"<key>"` の**直後**（空白と colon を跨いだ先）の位置を返す。
fn find_key(body: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let mut from = 0_usize;
    while let Some(hit) = body.get(from..)?.find(&needle) {
        let at = from.saturating_add(hit).saturating_add(needle.len());
        let rest = body.get(at..)?;
        let after = rest.trim_start();
        if let Some(value) = after.strip_prefix(':') {
            let skipped = rest.len().saturating_sub(value.len());
            return Some(at.saturating_add(skipped));
        }
        from = at;
    }
    None
}

/// `"<key>": "<value>"` の対が在るか（空白に寛容）。
fn has_pair(body: &str, key: &str, value: &str) -> bool {
    find_key(body, key)
        .and_then(|at| body.get(at..))
        .and_then(quoted_value)
        .is_some_and(|found| found == value)
}

/// colon の後ろの `"..."` の中身（escape は考えない＝status は識別子である）。
fn quoted_value(after_colon: &str) -> Option<&str> {
    let opened = after_colon.trim_start().strip_prefix('"')?;
    opened.split_once('"').map(|(value, _)| value)
}

/// colon の後ろの `{ ... }` の**直下**だけ（入れ子の object に入ったら打ち切る）。
fn immediate_object(after_colon: &str) -> Option<&str> {
    let opened = after_colon.trim_start().strip_prefix('{')?;
    let end = opened
        .find(['{', '}'])
        .unwrap_or(opened.len());
    opened.get(..end)
}

/// 前提違反（rc 1 + stderr 1 行・何もしない）。
fn refused(reason: String) -> Outcome {
    Outcome::failed_line(RC_REFUSED, format!("runner: {reason}"))
}
